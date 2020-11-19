use std::path::PathBuf;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::process::Command;

use structopt::StructOpt;
use anyhow::{Context, Result};
use log::debug;
use simple_logger::SimpleLogger;
use serde::{Serialize, Deserialize};
use json_comments::StripComments;

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
    windowsTerminalProfileId: Option<String>
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
    let dockerwsl_path = pull.dockerwsl;
    
    let mut dockerwsl_content = parse_dockerwslconf_file(&dockerwsl_path)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;

    let pull_wsl = &pull.wsl;
    let acr_conf = &dockerwsl_content.acr;

    for wsl_conf in dockerwsl_content.wsls.iter_mut() {
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

        let (registry_name, repository_name, tag) = extract_image_details(wsl_conf.image.as_str()).unwrap();

        handle_pull_for_image(registry_name.as_str(), 
                              repository_name.as_str(), 
                              acr_conf.username.as_str(), 
                              acr_conf.password.as_str(), 
                              acr_conf.tenant.as_str(), 
                              wsl_conf).with_context(|| format!("Could not handle pull for WSL `{}`!", &wsl_conf.name))?;
        
        //  debug!("{:#?}",az_login_command_status); 
    }

    write_dockerwsl_file(&dockerwsl_path, &dockerwsl_content)
        .with_context(|| format!("An error occurred while writing to the .dockerwsl file the updates from the pull subcommand!"))?;

    Ok(())
}

fn extract_image_details(image_url: &str) -> Result<(String, String, String)> {
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

    Ok((String::from(registry_name), String::from(repository_name), String::from(tag)))
}

fn handle_pull_for_image(registry_name: &str, repository_name: &str, username: &str, password: &str, tenant: &str, wsl_conf: &mut WSLConf) -> Result<String> {
    let latest_tag = get_latest_tag(registry_name, repository_name, username, password, tenant)
        .with_context(|| format!("Could not get latest tag for repository {}.azurecr.io/{}", registry_name, repository_name))?;

    let latest_tag_str = latest_tag.as_str();
    if wsl_conf.latest.is_none() || wsl_conf.latest.as_ref().unwrap() != latest_tag_str {
        pull_latest_image_tag(registry_name, repository_name, latest_tag_str, username, password)
            .with_context(|| format!("An error ocurred while pulling image from registry!"))?;

        wsl_conf.latest = Option::from(String::from(latest_tag_str));
        println!("WSL config with name `{}` will be updated with the new latest tag `{}` in .dockerwsl file.", &wsl_conf.name, latest_tag_str);
    }

    Ok(latest_tag)
}

fn handle_upgrade(upgrade: Upgrade) -> Result<()> {
    let mut dockerwsl_content = parse_dockerwslconf_file(&upgrade.dockerwsl)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &upgrade.dockerwsl))?;

    let upgrade_wsl = &upgrade.wsl;
    let acr_conf = &dockerwsl_content.acr;

    for mut wsl_conf in dockerwsl_content.wsls.iter_mut() {
        match upgrade_wsl {
            Some(name) => {
                if name.ne(&wsl_conf.name) {
                    debug!("Passed wsl name (`{}`) doesn't match current config entry name, will SKIP it!", &name);
                    continue;
                } else {
                    debug!("Passed wsl name (`{}`) matches current config entry name, will process it!", &name);
                }
            },
            None => { debug!("No wsl name passed to `upgrade`, will upgrade all wsls in the config file!"); }
        }

        let (registry_name, repository_name, tag) = extract_image_details(&wsl_conf.image).unwrap();

        if wsl_conf.latest.is_none() {
            handle_pull_for_image(registry_name.as_str(), 
                repository_name.as_str(), 
                acr_conf.username.as_str(), 
                acr_conf.password.as_str(), 
                acr_conf.tenant.as_str(), 
                wsl_conf).with_context(|| format!("Could not handle pull for WSL `{}`!", &wsl_conf.name))?;
        } 

        let latest_tag = wsl_conf.latest.as_ref().unwrap();
        // Steps
        // 1. Create/Check if WSL VM with name_latest_tag exists
        let wsl_vm_name = format!("{}-{}", &wsl_conf.name, latest_tag.as_str());
        create_wsl_vm(registry_name.as_str(), repository_name.as_str(), &wsl_conf.name, latest_tag.as_str())
            .with_context(|| format!("Could not create WSL VM for WSL `{}` and tag `{}`", &wsl_conf.name, &tag))?;

        if wsl_conf.windowsTerminalProfileId.is_none() {
            wsl_conf.windowsTerminalProfileId = Option::from(uuid::Uuid::new_v4().to_hyphenated().to_string());
        }

        let wt_profile_guid = wsl_conf.windowsTerminalProfileId.as_ref().with_context(|| format!("Unexpected error occurred, a UUID should've been allocated for the `{}` Windows Terminal profile!", &wsl_conf.name))?;

        // 2. Create/Update profile in Windows Terminal settings file
        create_windows_terminal_profile(&upgrade.wtconfig, wt_profile_guid.as_str(), &wsl_conf.name, wsl_vm_name.as_str())
            .with_context(|| format!("Could not create/update windows terminal profile for WSL `{}`!", &wsl_conf.name))?;
        // 3. Update image url and profile GUID in .dockerwsl conf with the latest tag
        update_wsl_info(&upgrade.dockerwsl, &wsl_conf.name, wt_profile_guid.as_str(), latest_tag.as_str())
            .with_context(|| format!("Could not update WSL information after upgrading WSL `{}`", &wsl_conf.name))?;
        //  debug!("{:#?}",az_login_command_status); 
    }

    write_dockerwsl_file(&upgrade.dockerwsl, &dockerwsl_content)
        .with_context(|| format!("An error occurred while writing to the .dockerwsl file the updates from the pull subcommand!"))?;

    
    Ok(())
}

fn update_wsl_info(dockerwsl_path: &PathBuf, wsl_name: &str, wt_profile_guid: &str, latest_tag: &str) -> Result<()> {
    
    Ok(())
}

fn create_wsl_vm(registry_name: &str, repository_name: &str, wsl_name: &str, tag: &str) -> Result<()> {

    Ok(())
}


fn create_windows_terminal_profile(windows_terminal_config_path: &PathBuf, wt_profile_guid: &str, wsl_name: &str) -> Result<()> {
    let mut wt_config_content = parse_json_file_without_comments(windows_terminal_config_path)
        .with_context(|| format!("Could not parse Window Terminal settings file `{:#?}`!", windows_terminal_config_path))?;
    
    let wt_profiles = wt_config_content.get_mut("profiles")
        .with_context(|| format!("Windows Terminal settings file `{:#?}` doesn't have a `profiles` property!", windows_terminal_config_path))?;
  
    let wt_profiles_list = wt_profiles.get_mut("list")
        .with_context(|| format!("Windows Terminal settings file `{:#?}` doesn't have a `profiles.list` property!", windows_terminal_config_path))?;

    let wt_profiles_list_array = wt_profiles_list.as_array_mut()
        .with_context(|| format!("Syntax incorrect for `profiles.list` array property in Windows Terminal settings file `{:#?}`", windows_terminal_config_path))?;

    let wt_profiles_list_array_filter = |profile: &&mut serde_json::Value| {
        let profile_object = profile.as_object();
        match profile_object {
            Some(p_obj) => {
                let guid = p_obj.get("guid");
                match guid {
                    Some(guid_value) => {
                        if guid_value.is_string() && guid_value.as_str().unwrap() == wt_profile_guid {
                            return true;
                        } else {
                            return false;
                        }
                    },
                    None => { return false; }
                }
            },
            None => { return false; }
        }
    };

    let wt_profile = wt_profiles_list_array.iter_mut().find(wt_profiles_list_array_filter);
    
    match wt_profile {
        Some(profile) => { },
        None => {
            let profile_object= serde_json::json!(
            {
                "guid": format!("{{{}}}", wt_profile_guid),
                "hidden": false,
                "name": wsl_name,
                "commandLine": format!("dragon run -w {}", wsl_name)
            });

            wt_profiles_list_array.insert(0, profile_object);
        }
    }

    write_json_file(windows_terminal_config_path, &wt_config_content)
        .with_context(|| format!("Could not write Windows Terminal settings file `{:#?}`!", windows_terminal_config_path))?;

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

fn parse_json_file_without_comments(file_path: &PathBuf) -> Result<serde_json::Value> {
    let file_path_str = file_path.to_str().unwrap();
    debug!("Attempting to parse json file `{}` (comments will be removed).", file_path_str);

    let file_content_str = std::fs::read_to_string(file_path)
        .with_context(|| format!("Could not read json file `{}`", file_path_str))?;

    let file_reader = StripComments::new(file_content_str.as_bytes());
    let file_content: serde_json::Value = serde_json::from_reader(file_reader).with_context(|| "Could not parse json file!")?;

    debug!("File `{}` was parsed successfully!", file_path_str);
    Ok(file_content)
}

fn write_json_file(file_path: &PathBuf, json_content: &serde_json::Value) -> Result<()> {
    let file_path_str = file_path.to_str().unwrap();
    debug!("Attempting to write to json file `{}`.", file_path_str);

    debug!("{:#?}",json_content); 

    let file =  OpenOptions::new().write(true).truncate(true).create(true).open(file_path)
        .with_context(|| format!("Could not open json file `{}` for writing!", file_path_str))?;
    let file_writer = BufWriter::new(file);
    serde_json::to_writer_pretty(file_writer, json_content).with_context(|| format!("An error happened while writing to json file `{}`", file_path_str))?;

    debug!("File `{}` was updated successfully!", file_path_str);
    Ok(())
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

