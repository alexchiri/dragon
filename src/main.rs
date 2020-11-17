use std::path::{PathBuf, Path};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::process::{Command, ExitStatus};

use structopt::StructOpt;
use anyhow::{Context, Result};
use log::{info, warn, debug, trace};
use simple_logger::{SimpleLogger};
use serde_yaml::Value;
use serde::{Serialize, Deserialize};

#[derive(Debug, StructOpt)]
#[structopt(name = "dragon", about = "A CLI tool that manages Docker generated WSL2 VMs and Windows Terminal profiles.")]
struct Dragon {
    #[structopt(subcommand)]
    command: SubCommand,

    /// Control verbosity of the output. Valid options -v, -vv, -vvv.
    #[structopt(flatten)]
    verbose: clap_verbosity_flag::Verbosity
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    /// Pulls the latest Docker image(s) from registry and update latest property in .dockerwsl file
    Pull(Pull),
    /// Creates new WSL VM with the latest version (if doesn't exist) and update Windows Terminal config.
    Upgrade(Upgrade)
}

#[derive(Debug, StructOpt)]
struct Upgrade {
    /// Path to the .dockerwsl file. Mandatory.
    #[structopt(short = "c", long, parse(from_os_str))]
    dockerwsl: PathBuf,
    /// Path to the Windows Terminal configuration file. Mandatory.
    #[structopt(short = "t", long, parse(from_os_str))]
    wtconfig: PathBuf,
    /// Which WSL VM would you like to change? Provide its name as configured in .dockerwsl.
    /// Optional, if not provided, all WSLs are gonna be affected. 
    #[structopt(short = "w", long)]
    wsl: Option<String>,
}
#[derive(Debug, StructOpt)]
struct Pull {
    /// Path to the .dockerwsl file. Mandatory.
    #[structopt(short = "c", long, parse(from_os_str))]
    dockerwsl: PathBuf,
    /// Which WSL VM would you like to change? Provide its name as configured in .dockerwsl.
    /// Optional, if not provided, all WSLs are gonna be affected. 
    #[structopt(short = "w", long)]
    wsl: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
struct DockerWSLConf {
    wsls: Vec<WSLConf>,
    acr: ACRConf
}

#[derive(Debug, Serialize, Deserialize)]
struct ACRConf {
    username: String,
    password: String,
    tenant: String
}
#[derive(Debug, Serialize, Deserialize)]
struct WSLConf {
    name: String,
    image: String,
    latest: Option<String>,
    windowsTerminalProfileId: String
}

fn main() -> Result<()> {
    let dragon_params = Dragon::from_args();
    SimpleLogger::new().with_level(dragon_params.verbose.log_level().unwrap().to_level_filter()).init();
    
    debug!("{:#?}", dragon_params);

    match dragon_params.command {
        SubCommand::Pull(pull_command) => {
            debug!("Received a Pull command: {:#?}", pull_command);
            return handle_pull(pull_command);
        }

        SubCommand::Upgrade(upgrade_command) => {
            debug!("Received an Upgrade command: {:#?}", upgrade_command);
            return handle_upgrade(upgrade_command);
        }
    }

}

fn handle_pull(pull: Pull) -> Result<()> {
    let dockerwsl_path = PathBuf::from(pull.dockerwsl);
    
    let mut dockerwsl_content = parse_dockerwslconf_file(&dockerwsl_path)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;

    let pull_wsl = &pull.wsl;
    let acr_conf = &dockerwsl_content.acr;

    for mut wsl_conf in dockerwsl_content.wsls.iter_mut() {
        match pull_wsl {
            Some(name) => {
                if name.ne(&wsl_conf.name) {
                    debug!("Passed wsl name (`{}`) doesn't match current config entry name, will SKIP it!", &name);
                    continue;
                } else {
                    debug!("Passed wsl name (`{}`) matches current config entry name, will process it!", &name);
                }
            },
            None => { debug!("No wsl name passed to `pull`, will pull all wsls in the config file!"); }
        }

        let (registry_name, repository_name, tag) = extract_image_details(&wsl_conf.image).unwrap();

        let latest_tag = get_latest_tag(registry_name, 
                                                repository_name, 
                                                acr_conf.username.as_str(), 
                                                acr_conf.password.as_str(), 
                                                acr_conf.tenant.as_str()).with_context(|| format!(""))?;

        let latest_tag_str = latest_tag.as_str();
        if wsl_conf.latest.is_none() || wsl_conf.latest.as_ref().unwrap() != latest_tag_str {
            pull_latest_image_tag(registry_name, repository_name, latest_tag_str, acr_conf.username.as_str(), acr_conf.password.as_str())
                .with_context(|| format!("An error ocurred while pulling image from registry!"))?;

            wsl_conf.latest = Option::from(String::from(latest_tag_str));
            println!("WSL config with name `{}` has been updated with the new latest tag `{}` in .dockerwsl file `{}`.", &wsl_conf.name, latest_tag_str, &dockerwsl_path.to_str().unwrap());
        }

        
        //  debug!("{:#?}",az_login_command_status); 
    }

    write_dockerwsl_file(&dockerwsl_path, &dockerwsl_content)
        .with_context(|| format!("An error occurred while writing to the .dockerwsl file the updates from the pull subcommand!"))?;

    Ok(())
}

fn handle_upgrade(upgrade: Upgrade) -> Result<()> {

    Ok(())
}

fn pull_latest_image_tag(registry_name: &str, repository_name: &str, tag: &str, username: &str, password: &str) -> Result<()> {
    let registry_url = format!("{}.azurecr.io", registry_name);
    let registry_url_str = registry_url.as_str();

    let mut docker_login_command = Command::new(r#"docker"#);
    docker_login_command.args(&["login", registry_url_str])
                    .args(&["--username", username])
                    .args(&["--password", password]);

    let docker_login_command_status = docker_login_command.status()
        .with_context(|| format!("`docker login {}` failed!", registry_url_str))?;
    if !docker_login_command_status.success() {
        return Err(anyhow::anyhow!("`docker login {}` failed. Double-check the service principal details in `.dockerwsl`!", registry_url_str));
    }

    let image_url = format!("{}/{}:{}", registry_url, repository_name, tag);
    let image_url_str = image_url.as_str();
    let mut docker_pull_command = Command::new(r#"docker"#);
    docker_pull_command.args(&["pull", image_url_str]);

    let docker_pull_command_status = docker_pull_command.status()
        .with_context(|| format!("`docker pull {}` failed!", image_url_str))?;
    if !docker_pull_command_status.success() {
        return Err(anyhow::anyhow!("`docker pull {}` failed!", image_url_str));
    }
    
    Ok(())
}

fn extract_image_details(image_url: &str) -> Result<(&str, &str, &str)> {
    let regex = regex::Regex::new(r"(.+?)\.azurecr\.io/(.+?):(.+?)").unwrap();
    let image_regex_captures = regex.captures(image_url)
        .with_context(|| format!("Docker image property does not have the expected format `registry.azurecr.io/repository:tag`!"))?;

    let registry_name = image_regex_captures.get(1)
        .with_context(|| format!("Could not extract registry name from Docker image URL `{}`!", image_url))?
        .as_str();
    let repository_name = image_regex_captures.get(2)
        .with_context(|| format!("Could not extract repository name from Docker image URL `{}`!", image_url))?
        .as_str();
    let tag = image_regex_captures.get(3)
        .with_context(|| format!("Could not extract tag from Docker image URL `{}`!", image_url))?
        .as_str();

    Ok((registry_name, repository_name, tag))
}

fn parse_yaml_file(file_path: &PathBuf) -> Result<Value> {
    let file_path_str = file_path.to_str().unwrap();

    debug!("Attempting to parse yaml file `{}`.", file_path_str);

    let file = File::open(file_path)
        .with_context(|| format!("Could not open file `{}`", file_path_str))?;
    let file_reader = BufReader::new(file);
    let file_content: Value = serde_yaml::from_reader(file_reader).with_context(|| "Could not parse yaml file!")?;

    debug!("File `{}` was parsed successfully!", file_path_str);
    Ok(file_content)
}

fn parse_dockerwslconf_file(file_path: &PathBuf) -> Result<DockerWSLConf> {
    let file_path_str = file_path.to_str().unwrap();

    debug!("Attempting to parse `.dockerwsl` conf file `{}`.", file_path_str);

    let file = File::open(file_path)
        .with_context(|| format!("Could not open .dockerwsl file `{}` for reading!", file_path_str))?;
    let file_reader = BufReader::new(file);
    let file_content: DockerWSLConf = serde_yaml::from_reader(file_reader).with_context(|| "Could not parse yaml file!")?;

    debug!("`.dockerwsl` file `{}` was parsed successfully!", file_path_str);
    Ok(file_content)
}

fn write_dockerwsl_file(file_path: &PathBuf, dockerwsl_conf: &DockerWSLConf) -> Result<()> {
    let file_path_str = file_path.to_str().unwrap();
    debug!("Attempting to update .dockerwsl file `{}`.", file_path_str);

    debug!("{:#?}",dockerwsl_conf); 

    let file =  OpenOptions::new().write(true).open(file_path)
        .with_context(|| format!("Could not open .dockerwsl file `{}` for writing!", file_path_str))?;
    let file_writer = BufWriter::new(file);
    serde_yaml::to_writer(file_writer, dockerwsl_conf).with_context(|| format!("An error happened while writing .dockerwsl file to `{}`", file_path_str))?;

    debug!("File `{}` was updated successfully!", file_path_str);
    Ok(())
}

fn az_login(username: &str, password: &str, tenant: &str) -> Result<()> {
    let mut az_login_command = Command::new(r#"C:\Program Files (x86)\Microsoft SDKs\Azure\CLI2\wbin\az.cmd"#);
    az_login_command.args(&["login", "--service-principal"])
                    .args(&["--username", username])
                    .args(&["--password", password])
                    .args(&["--tenant", tenant]);

    let az_login_command_status = az_login_command.status()
        .with_context(|| format!("`az login --service-principal` failed!"))?;
    if !az_login_command_status.success() {
        return Err(anyhow::anyhow!("`az login --service-principal` failed. Double-check the service principal details in `.dockerwsl`!"));
    }
    
    Ok(())
}

fn get_latest_tag(registry_name:&str, repository_name: &str, username: &str, password: &str, tenant: &str) -> Result<String> {
    az_login(username, password, tenant).with_context(|| format!("There was an error while logging in to Azure!"))?;
    
    let mut az_get_latest_tag_command = Command::new(r#"C:\Program Files (x86)\Microsoft SDKs\Azure\CLI2\wbin\az.cmd"#);
    az_get_latest_tag_command.args(&["acr", "repository", "show-manifests"])
                             .args(&["-n", registry_name])
                             .args(&["--repository", repository_name])
                             .args(&["--orderby", "time_desc"])
                             .args(&["--top", "1"])
                             .args(&["--query", "[0].tags[0]"]);
    let az_get_latest_tag_command_output = az_get_latest_tag_command.output()
        .with_context(|| format!("Failed to retrieve the latest tag for {}.azurecr.io/{}!", registry_name, repository_name))?;
    
    let az_latest_tag_output = String::from_utf8(az_get_latest_tag_command_output.stdout)
        .with_context(|| format!("Could not convert latest tag to UTF-8 string!"))?;

    let image_tag_regex = regex::Regex::new(r#""(.+?)"\r\n"#).unwrap();
    let image_tag_captures = image_tag_regex.captures(az_latest_tag_output.as_str())
            .with_context(|| format!("Docker image tag does not have the expected format!"))?;
    let latest_tag = image_tag_captures.get(1)
        .with_context(|| format!("Could not extract latest tag from az CLI output `{}`!", az_latest_tag_output.as_str()))?
        .as_str();

    return Ok(String::from(latest_tag));
}

