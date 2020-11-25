use std::path::{PathBuf, Path};
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::{BufReader, BufWriter};
use std::process::Command;

use structopt::StructOpt;
use anyhow::{Context, Result};
use log::debug;
use simple_logger::SimpleLogger;
use serde::{Serialize, Deserialize};
use json_comments::StripComments;
use tempfile::{Builder, TempDir};
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

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
    /// Pulls the Docker image(s) from registry using the provided image URL
    Pull(Pull),
    /// Creates new WSL VM with the latest version (if doesn't exist) and updates .dockerwsl image url to contain the latest tag.
    Upgrade(Upgrade),
    /// Creates .dockerwsl entry from image url, pulls image, creates WSL VM and creates record in Windows Terminal settings.json file.
    New(New),
    /// Only for ACR based images. Determines the latest tag for a repository and updates the latest property in .dockerwsl file
    Update(Update),
    /// Runs a configured and existing WSL VM by name.
    Run(Run)

    // Test(Test)
}

#[derive(Debug, StructOpt)]
struct Upgrade {
    /// Path to the .dockerwsl file. Mandatory.
    #[structopt(short = "c", long, parse(from_os_str), env="DOCKERWSL_PATH")]
    dockerwsl: PathBuf,
    /// Path to the Windows Terminal configuration file. Mandatory.
    #[structopt(short = "t", long, parse(from_os_str), env="WT_SETTINGS_PATH")]
    wtconfig: PathBuf,
    /// Which WSL VM would you like to change? Provide its name as configured in .dockerwsl.
    /// Optional, if not provided, all WSLs are gonna be affected. 
    #[structopt(short = "w", long)]
    wsl: Option<String>,
}
#[derive(Debug, StructOpt)]
struct Pull {
    /// Path to the .dockerwsl file. Mandatory.
    #[structopt(short = "c", long, parse(from_os_str), env="DOCKERWSL_PATH")]
    dockerwsl: PathBuf,
    /// Which WSL VM would you like to change? Provide its name as configured in .dockerwsl.
    /// Optional, if not provided, all WSLs are gonna be affected. 
    #[structopt(short = "w", long)]
    wsl: Option<String>,
}

#[derive(Debug, StructOpt)]
struct New {
    /// Path to the .dockerwsl file. Mandatory.
    #[structopt(short = "c", long, parse(from_os_str), env="DOCKERWSL_PATH")]
    dockerwsl: PathBuf,
    /// Path to the Windows Terminal configuration file. Mandatory.
    #[structopt(short = "s", long, parse(from_os_str), env="WT_SETTINGS_PATH")]
    wtconfig: PathBuf,
    /// Image URL, can be given as registry/repository:tag, or registry/repository or just repository[:tag] if using image from Docker Hub
    #[structopt(short = "i", long)]
    image: String,
    /// The name of the WSL that is used in the VM name and in the .dockerwsl config.
    /// Optional, if not provided, repository name will be used
    #[structopt(short = "w", long)]
    name: Option<String>,
    /// Path to the folder where the WSL VMs folders will be created for this particular WSL.
    /// Optional, but only if `default_base_wsl_install_path` is configured in `.dockerwsl` file
    #[structopt(short = "l", long = "install-location", parse(from_os_str))]
    base_install_path: Option<PathBuf>,
    /// Username to be used for private registry. Optional
    #[structopt(short = "u", long)]
    username: Option<String>,
    /// Password to be used for private registry. Optional
    #[structopt(short = "p", long)]
    password: Option<String>,
    /// Tenant to be used for ACR registries only. Optional
    #[structopt(short = "t", long)]
    tenant: Option<String>
}

#[derive(Debug, StructOpt)]
struct Update {
    /// Path to the .dockerwsl file. Mandatory.
    #[structopt(short = "c", long, parse(from_os_str), env="DOCKERWSL_PATH")]
    dockerwsl: PathBuf,
    
    /// Which WSL VM would you like to change? Provide its name as configured in .dockerwsl.
    /// Optional, if not provided, all WSLs are gonna be affected. 
    #[structopt(short = "w", long)]
    wsl: Option<String>,
}

#[derive(Debug, StructOpt)]
struct Test {
    
}

#[derive(Debug, StructOpt)]
struct Run {
    /// Path to the .dockerwsl file. Mandatory.
    #[structopt(short = "c", long, parse(from_os_str), env="DOCKERWSL_PATH")]
    dockerwsl: PathBuf,
    /// Which WSL VM would you like to run? Provide its name as configured in .dockerwsl. Mandatory.
    #[structopt(short = "w", long)]
    wsl: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DockerWSLConf {
    wsls: Vec<WSLConf>,
    default_base_wsl_install_path: Option<String>,
    private_registries: Vec<Registry>
}

#[derive(Debug, Serialize, Deserialize)]
struct Registry {
    name: String,
    username: String,
    password: String,
    tenant: Option<String>
}
#[derive(Debug, Serialize, Deserialize)]
struct WSLConf {
    name: String,
    image: String,
    latest: Option<String>,
    windows_terminal_profile_id: String,
    base_install_path: String
}

fn main() -> Result<()> {
    let dragon_params = Dragon::from_args();
    SimpleLogger::new().with_level(dragon_params.verbose.log_level().unwrap().to_level_filter()).init()
        .with_context(|| format!("Could not initialize logging!"))?;
    
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

        SubCommand::New(new_command) => {
            debug!("Received a New command: {:#?}", new_command);
            return handle_new(new_command);
        }

        SubCommand::Run(run_command) => {
            debug!("Received a Run command: {:#?}", run_command);
            return handle_run(run_command);
        }

        SubCommand::Update(update_command) => {
            debug!("Received an Update command: {:#?}", update_command);
            return handle_update(update_command);
        }

        // SubCommand::Test(test_command) => {
        //     debug!("Received a Test command: {:#?}", test_command);
        //     return handle_test(test_command);
        // }
    }

}

// fn handle_test(test: Test) -> Result<()> {    
//     let mut wsl_run_command = Command::new(r#"wsl"#);
//     wsl_run_command.args(&["-d", "nginx-latest"]);

//     let wsl_run_command_status = wsl_run_command.status()
//         .with_context(|| format!("`wsl -d nginx-latest` failed!"))?;

//     if !wsl_run_command_status.success() {
//         return Err(anyhow::anyhow!("Could not run WSL VM `nginx-latest`!"));
//     }
    
//     Ok(())
// }

fn handle_update(update: Update) -> Result<()> {
    let dockerwsl_path = &update.dockerwsl;
    let wsl_name = &update.wsl;
    
    let mut dockerwsl_content = parse_dockerwslconf_file(&dockerwsl_path)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;

    for wsl_conf in dockerwsl_content.wsls.iter_mut() {
        match wsl_name {
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

        let image_url_str = wsl_conf.image.as_str();

        let (registry_name_option, repository_name, _tag) = extract_generic_image_details(image_url_str)
            .with_context(|| format!("Could not extract Docker image details from URL `{}`!", image_url_str))?;

        if registry_name_option.is_none() {
            return Err(anyhow::anyhow!("`dragon update` is only supported for ACR images!"));
        } else {
            let registry_name = registry_name_option.unwrap();
            
            if !registry_name.ends_with(".azurecr.io") {
                return Err(anyhow::anyhow!("`dragon update` is only supported for ACR images!"));
            }

            let registry_name_str = registry_name.as_str();

            let private_registry_option = dockerwsl_content.private_registries.iter().find(|reg| reg.name.as_str() == registry_name_str);

            if private_registry_option.is_some() {
                let private_registry = private_registry_option.unwrap();

                if private_registry.tenant.is_none() {
                    return Err(anyhow::anyhow!("Tenant is required in order to determine the most recent tag for the image `{}`!", &wsl_conf.name));
                }

                let username_str = private_registry.username.as_str();
                let password_str = private_registry.password.as_str();
                let tenant = private_registry.tenant.as_ref().unwrap();
                let tenant_str = tenant.as_str();
                let repository_name_str = repository_name.as_str();

                let latest_tag = get_latest_tag(registry_name_str, repository_name_str, username_str, password_str, tenant_str)
                    .with_context(|| format!("Could not get latest tag for repository {}.azurecr.io/{}", registry_name_str, repository_name_str))?;
                
                let latest_tag_str = latest_tag.as_str();
                wsl_conf.latest = Some(latest_tag.clone());
                println!("WSL `{}` latest property will be updated to `{}`!", &wsl_conf.name, latest_tag_str);
            }
        }
    }

    write_dockerwsl_file(dockerwsl_path, &dockerwsl_content)
        .with_context(|| format!("Could not write `.dockerwsl` file `{:#?}`!", dockerwsl_path))?;
    
    Ok(())
}

fn handle_run(run: Run) -> Result<()> {
    let dockerwsl_path = &run.dockerwsl;
    let dockerwsl_content = get_dockerwsl_content(dockerwsl_path)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;
    let wsl_name = &run.wsl;

    let wslconf_option = dockerwsl_content.wsls.iter().find(|wsl| wsl.name == wsl_name.as_str());

    if wslconf_option.is_none() {
        return Err(anyhow::anyhow!("Could not find .dockerwsl config with name `{}`!", wsl_name));
    } 

    let wsl_conf = wslconf_option.unwrap();
    let image_url_string = &wsl_conf.image;

    let (_registry_name, _repository_name, tag) = extract_generic_image_details(image_url_string.as_str())
        .with_context(|| format!("Could not extract Docker image details from URL `{}`!", image_url_string))?;

    if tag.is_none() {
        return Err(anyhow::anyhow!("Could not find image tag in image URL `{}` in .dockerwsl file for WSL `{}`!", image_url_string, wsl_name));
    }

    let tag_str = tag.unwrap();
    let wsl_vm_name = get_wsl_wm_name(wsl_name.as_str(), tag_str.as_str())
        .with_context(|| format!("Could not compose WSL VM name from WSL name and tag!"))?;

    let mut wsl_run_command = Command::new(r#"wsl"#);
    wsl_run_command.args(&["-d", &wsl_vm_name]);

    let wsl_run_command_status = wsl_run_command.status()
        .with_context(|| format!("`wsl -d {}` failed!", &wsl_vm_name))?;

    if !wsl_run_command_status.success() {
        return Err(anyhow::anyhow!("Could not run WSL VM `{}`!", wsl_vm_name));
    }
    
    Ok(())
}

fn get_wsl_wm_name(wsl_name: &str, tag: &str) -> Result<String> {
    return Ok(format!("{}-{}", wsl_name, tag));
}

fn wsl_vm_exists(wsl_name: &str) -> Result<bool> {
    let mut wsl_list_command = Command::new(r#"wsl"#);
    wsl_list_command.args(&["-l", "-q"]);

    let wsl_list_command_output = wsl_list_command.output()
        .with_context(|| format!("`wsl -l -q` failed!"))?;

    let stdout_string = String::from_utf8(wsl_list_command_output.stdout)
        .with_context(|| format!("Couldn't parse stdout!"))?;

    let stdout_wsl_list: Vec<&str> = stdout_string.split(format!("\r{}\n{}", char::from(0), char::from(0)).as_str()).collect();

    let existing_wsl = stdout_wsl_list.iter().find(|w| {
        w.replace(char::from(0), "") == wsl_name
    });

    if existing_wsl.is_some() { return Ok(true); }
    else { return Ok(false); }
}

fn handle_new(new: New) -> Result<()> {
    let mut image_url = new.image;
    let (registry_name_option, repository_name, tag_option) = extract_generic_image_details(image_url.as_str())
        .with_context(|| format!("Could not extract Docker image details from URL `{}`!", image_url.as_str()))?;

    if tag_option.is_none() {
        image_url = format!("{}:latest", image_url);
    }

    let wsl_name = new.name.unwrap_or(repository_name.clone());
    let wsl_name_str = wsl_name.as_str();

    let wt_profile_id = uuid::Uuid::new_v4().to_hyphenated().to_string();

    let dockerwsl_path = &new.dockerwsl;

    if registry_name_option.is_some() && new.username.is_some() && new.password.is_some() {
        let registry_name = registry_name_option.clone().unwrap();
        let registry_name_str = registry_name.as_str();
        let username = new.username.unwrap();
        let username_str = username.as_str();
        let password = new.password.unwrap();
        let password_str = password.as_str();

        create_private_registry_record(registry_name_str, username_str, password_str, new.tenant, dockerwsl_path)
            .with_context(|| format!("Could not create private registry record in .dockerwsl for `{}`!", registry_name_str))?;
    }

    handle_pull_for_image(registry_name_option, dockerwsl_path, image_url.as_str())
        .with_context(|| format!("Could not handle pull for image `{}`!", image_url.as_str()))?;

    let tag = tag_option.unwrap_or("latest".to_string());
    let wsl_vm_name = get_wsl_wm_name(repository_name.as_str(), tag.as_str())
        .with_context(|| format!("Could not compose WSL VM name from WSL name and tag!"))?;
    let wsl_vm_name_str = wsl_vm_name.as_str();

    let base_install_path = determine_base_install_path(&new.base_install_path, dockerwsl_path, wsl_name_str)
        .with_context(|| format!("Could not determine base install path for WSL VM `{}`!", wsl_vm_name_str))?;

    let temp_dir = Builder::new().prefix("dragon").tempdir()?;
    let tar_path = export_docker_image_to_tar(image_url.as_str(), &temp_dir)
        .with_context(|| format!("Could not export docker image `{}` to tar file!", image_url.as_str()))?;

    create_wsl_vm_from_tar(wsl_vm_name_str, &tar_path, &base_install_path)
        .with_context(|| format!("Could not create WSL VM with name `{}`", wsl_vm_name_str))?;

    create_dockerwsl_config_entry(dockerwsl_path, image_url.as_str(), wsl_name_str, wt_profile_id.as_str(), &base_install_path, tag.as_str())
        .with_context(|| format!("Could not create the .dockerwsl config entry for the `{}` entry!", wsl_name_str))?;

    create_windows_terminal_profile(&new.wtconfig, wt_profile_id.as_str(), wsl_name_str)
        .with_context(|| format!("Could not create Windows Terminal profile in settings.json for `{}`!", wsl_name_str))?;

    Ok(())
}

fn create_private_registry_record(registry_name_str: &str, username: &str, password: &str, tenant: Option<String>, dockerwsl_path: &PathBuf) -> Result<()> {
    let mut dockerwsl_content = get_dockerwsl_content(dockerwsl_path)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;

    let private_registry_option = dockerwsl_content.private_registries.iter().find(|reg| reg.name.as_str() == registry_name_str);

    if private_registry_option.is_some() {
        println!("There are already credentials for private registry `{}! Will keep the existing ones.", registry_name_str);
    } else {
        let private_registry = Registry {
            name: registry_name_str.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            tenant: tenant
        };

        dockerwsl_content.private_registries.insert(0, private_registry);
    }

    write_dockerwsl_file(dockerwsl_path, &dockerwsl_content)
        .with_context(|| format!("Could not write `.dockerwsl` file `{:#?}`!", dockerwsl_path))?;

    println!("Credentials for private registry `{}` have been added to the .dockerwsl file!", registry_name_str);

    Ok(())
}

fn determine_login(registry_name_option: Option<String>, dockerwsl_path: &PathBuf) -> Result<()> {
    if registry_name_option.is_some() {
        let dockerwsl_content = get_dockerwsl_content(dockerwsl_path)
            .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;
        
        let registry_name_string = registry_name_option.unwrap();
        let registry_name_str = registry_name_string.as_str();

        let private_registry_option = dockerwsl_content.private_registries.iter().find(|reg| reg.name.as_str() == registry_name_str);

        if private_registry_option.is_some() {
            let private_registry = private_registry_option.unwrap();

            let username_str = private_registry.username.as_str();
            let password_str = private_registry.password.as_str();

            docker_login(registry_name_str, username_str, password_str)
                .with_context(|| format!("Could not `docker login` for registry `{}`", registry_name_str))?;
        }
    }

    Ok(())
}

fn determine_base_install_path(new_install_location: &Option<PathBuf>, dockerwsl_path: &PathBuf, wsl_name_str: &str) -> Result<PathBuf> {
    if new_install_location.is_none() {
        let dockerwsl_content = get_dockerwsl_content(dockerwsl_path)
            .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", dockerwsl_path))?;
        if dockerwsl_content.default_base_wsl_install_path.is_none() {
            return Err(anyhow::anyhow!("No install location was passed and no `default_wsl_install_location` value is defined in .dockerwsl file!"));
        } else {
            let parent_folder_wsl_path = PathBuf::from(dockerwsl_content.default_base_wsl_install_path.unwrap());
            let wsl_path = parent_folder_wsl_path.join(wsl_name_str);
            return Ok(wsl_path);
        }
    } else {
        return Ok(new_install_location.clone().unwrap());
    }
}

fn generate_rand_filename() -> Result<String> {
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(30)
        .collect();

    return Ok(rand_string);
}

fn export_docker_image_to_tar(image_url_str: &str, temp_dir: &TempDir) -> Result<PathBuf> {
    let docker_container_id = docker_create(image_url_str)
        .with_context(|| format!("Could not `docker create {}`!", image_url_str))?;
    let random_filename = generate_rand_filename()
        .with_context(|| format!("Could not generate a random filename!"))?;
    let tar_file_path = temp_dir.path().join(random_filename);

    docker_export(&docker_container_id, &tar_file_path)
        .with_context(|| format!("Could not export docker container with id `{}` to tar file `{:#?}`!", &docker_container_id, &tar_file_path))?;

    Ok(tar_file_path)
}

fn docker_create(image_url_str: &str) -> Result<String> {
    let mut docker_create_command = Command::new(r#"docker"#);
    docker_create_command.args(&["create", image_url_str]);

    let docker_create_command_output = docker_create_command.output()
        .with_context(|| format!("`docker create {}` failed!", image_url_str))?;

    let stdout_string = String::from_utf8(docker_create_command_output.stdout)
        .with_context(|| format!("Couldn't parse stdout!"))?;

    Ok(stdout_string.trim().replace(char::from(0), ""))
}

fn docker_export(docker_container_id: &str, tar_file_path: &PathBuf) -> Result<()> {
    let tar_file_path_str = tar_file_path.to_str()
        .with_context(|| format!("Could not convert path `{}` to &str!", tar_file_path.display()))?;

    let mut docker_container_export_command = Command::new(r#"docker"#);
    docker_container_export_command.args(&["container", "export"]);
    docker_container_export_command.args(&["-o", tar_file_path_str]);
    docker_container_export_command.arg(docker_container_id);

    let docker_container_export_command_status = docker_container_export_command.status()
        .with_context(|| format!("`docker container export -o {} {}` failed!", tar_file_path_str, docker_container_id))?;

    if !docker_container_export_command_status.success() {
        return Err(anyhow::anyhow!("Could not `docker container export -o {} {}`!", tar_file_path_str, docker_container_id));
    }

    Ok(())
}

fn create_dockerwsl_config_entry(dockerwsl_path: &PathBuf, image_url: &str, wsl_name: &str, wt_profile_id: &str, base_install_path: &PathBuf, latest_tag_str: &str) -> Result<()> {
    let mut dockerwsl_content = get_dockerwsl_content(&dockerwsl_path)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;

    let existing_dockerwsl = dockerwsl_content.wsls.iter().find(|wsl| wsl.name == wsl_name);

    if existing_dockerwsl.is_some() {
        return Err(anyhow::anyhow!("There is already a dockerwsl config with the name `{}`!", wsl_name));
    }

    let base_install_path_str = base_install_path.to_str().with_context(|| format!("Could not convert install path to &str!"))?;

    let wslconf = WSLConf {
        name: wsl_name.to_string(),
        image: image_url.to_string(),
        latest: Some(latest_tag_str.to_string()),
        base_install_path: format!("{}", base_install_path_str),
        windows_terminal_profile_id: wt_profile_id.to_string()
    };

    dockerwsl_content.wsls.insert(0, wslconf);

    write_dockerwsl_file(dockerwsl_path, &dockerwsl_content)
        .with_context(|| format!("Could not write `.dockerwsl` file `{:#?}`!", dockerwsl_path))?;

    println!("WSL config for `{}` has been added to the .dockerwsl file!", wsl_name);

    Ok(())
}

fn extract_generic_image_details(image_url: &str) -> Result<(Option<String>, String, Option<String>)> {
    let regex = regex::Regex::new(r"^(?:.*?(.+?)/)?([^:]+)(?::(.+))?$").unwrap();
    let image_regex_captures = regex.captures(image_url)
        .with_context(|| format!("Docker image property does not have the expected format `[registry/]repository[:tag]`!"))?;
    
    let registry_name = image_regex_captures.get(1).map_or(None, |m| Option::from(m.as_str().to_string()));
    let repository_name = image_regex_captures.get(2)
        .with_context(|| format!("Could not extract repository name from Docker image URL `{}`!", image_url))?
        .as_str();
    let tag = image_regex_captures.get(3).map_or(None, |m| Option::from(m.as_str().to_string()));

    Ok((registry_name, repository_name.to_string(), tag))
}

fn handle_pull(pull: Pull) -> Result<()> {
    let dockerwsl_path = &pull.dockerwsl;
    let wsl_name = &pull.wsl;
    
    let dockerwsl_content = parse_dockerwslconf_file(&dockerwsl_path)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &dockerwsl_path))?;

    for wsl_conf in dockerwsl_content.wsls.iter() {
        match wsl_name {
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

        let image_url_str = wsl_conf.image.as_str();

        let (registry_name, _repository_name, _tag) = extract_generic_image_details(image_url_str)
            .with_context(|| format!("Could not extract Docker image details from URL `{}`!", image_url_str))?;

        handle_pull_for_image(registry_name, dockerwsl_path, image_url_str)
            .with_context(|| format!("Could not handle pull for image `{}`!", image_url_str))?;
    }
   
    Ok(())
}

fn handle_pull_for_image(registry_name_option:Option<String>, dockerwsl_path:&PathBuf, image_url_str: &str) -> Result<()> {
    determine_login(registry_name_option, dockerwsl_path)
        .with_context(|| format!("Error occurred while determining if login is required for pulling docker image `{}`!", image_url_str))?;

    pull_image_tag(image_url_str)
        .with_context(|| format!("Could not pull the image {}!", image_url_str))?;
    
    Ok(())
}

fn handle_upgrade(upgrade: Upgrade) -> Result<()> {
    let mut dockerwsl_content = parse_dockerwslconf_file(&upgrade.dockerwsl)
        .with_context(|| format!("Could not parse `.dockerwsl` config file `{:#?}`!", &upgrade.dockerwsl))?;

    let upgrade_wsl = &upgrade.wsl;

    for wsl_conf in dockerwsl_content.wsls.iter_mut() {
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

        let (registry_name, repository_name, _tag) = extract_generic_image_details(&wsl_conf.image)
            .with_context(|| format!("Could not extract Docker image details from URL `{}`!", &wsl_conf.image))?;

        if wsl_conf.latest.is_none() {
            return Err(anyhow::anyhow!("There is no latest property in .dockerwsl for WSL `{}`! Either add the value manually or for images in ACR use `dragon update`.", &wsl_conf.name));
        } 

        let latest_tag = wsl_conf.latest.as_ref().unwrap();
        let updated_image_url = update_image_url(registry_name, &repository_name, latest_tag)
            .with_context(|| format!("Could not update image URL `{}` with the latest tag `{}`!", &wsl_conf.image, latest_tag))?;

        let wsl_vm_name = get_wsl_wm_name(repository_name.as_str(), latest_tag)
            .with_context(|| format!("Could not compose WSL VM name from WSL name and tag!"))?;
        let wsl_vm_name_str = wsl_vm_name.as_str();

        let temp_dir = Builder::new().prefix("dragon").tempdir()?;
        let tar_path = export_docker_image_to_tar(updated_image_url.as_str(), &temp_dir)
            .with_context(|| format!("Could not export docker image `{}` to tar file!", updated_image_url.as_str()))?;

        create_wsl_vm_from_tar(wsl_vm_name_str, &tar_path, &PathBuf::from(&wsl_conf.base_install_path))
            .with_context(|| format!("Could not create WSL VM with name `{}`", wsl_vm_name_str))?;

        create_windows_terminal_profile(&upgrade.wtconfig, wsl_conf.windows_terminal_profile_id.as_str(), &wsl_conf.name)
            .with_context(|| format!("Could not create Windows Terminal profile in settings.json for `{}`!", &wsl_conf.name))?;

        wsl_conf.image = updated_image_url;

        println!("WSL `{}` has been upgraded to tag `{}`!", &wsl_conf.name, latest_tag);
    }

    write_dockerwsl_file(&upgrade.dockerwsl, &dockerwsl_content)
        .with_context(|| format!("An error occurred while writing to the .dockerwsl file the updates from the pull subcommand!"))?;

    
    Ok(())
}

fn update_image_url(registry_name_option: Option<String>, repository_name_str:&String, latest_tag_str: &str) -> Result<String> {
    if registry_name_option.is_none() {
        return Ok(format!("{}:{}", repository_name_str, latest_tag_str));
    } else {
        return Ok(format!("{}/{}:{}", registry_name_option.unwrap(), repository_name_str, latest_tag_str));
    }
}

fn delete_wsl_vm(wsl_vm_name_str: &str) -> Result<()> {
    let mut wsl_unregister_command = Command::new(r#"wsl"#);
    wsl_unregister_command.args(&["--unregister", wsl_vm_name_str]);

    let wsl_unregister_command_status = wsl_unregister_command.status()
        .with_context(|| format!("`wsl --unregister {}` failed!", wsl_vm_name_str))?;

    if !wsl_unregister_command_status.success() {
        return Err(anyhow::anyhow!("Could not unregister `{}` WSL VM!", wsl_vm_name_str));
    }
    
    Ok(())
}

fn create_wsl_vm_from_tar(wsl_vm_name_str: &str, tar_path: &PathBuf, base_install_path: &PathBuf) -> Result<()> {
    let wsl_wm_exists_bool = wsl_vm_exists(wsl_vm_name_str)
        .with_context(|| format!("Could not verify if WSL VM `{}` already exists!", wsl_vm_name_str))?;

    if wsl_wm_exists_bool {
        delete_wsl_vm(wsl_vm_name_str)
            .with_context(|| format!("Could not existing WSL VM `{}`!", wsl_vm_name_str))?;
    }

    let tar_path_str = tar_path.to_str()
        .with_context(|| format!("Could not convert path `{}` to &str!", tar_path.display()))?;
    
    let install_path = base_install_path.join(wsl_vm_name_str);
    let install_path_str = install_path.to_str()
        .with_context(|| format!("Could not convert path `{}` to &str!", install_path.display()))?;
    create_dir_all(&install_path)
        .with_context(|| format!("Could not create the full install path `{}` for WSL VM `{}`!", install_path_str, wsl_vm_name_str))?;
    

    let mut wsl_import_command = Command::new(r#"wsl"#);
    wsl_import_command.arg("--import");
    wsl_import_command.arg(wsl_vm_name_str);
    wsl_import_command.arg(install_path_str);
    wsl_import_command.arg(tar_path_str);
    wsl_import_command.args(&["--version", "2"]);

    let wsl_import_command_status = wsl_import_command.status()
        .with_context(|| format!("`wsl --import {}` failed!", wsl_vm_name_str))?;

    if !wsl_import_command_status.success() {
        return Err(anyhow::anyhow!("Could not import `{}` WSL VM!", wsl_vm_name_str));
    }

    println!("WSL VM `{}` has been added!", wsl_vm_name_str);


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
    
    let wt_profile_guid_with_braces = format!("{{{ }}}", wt_profile_guid);

    let wt_profiles_list_array_filter = |profile: &&mut serde_json::Value| {
        let profile_object = profile.as_object();
        match profile_object {
            Some(p_obj) => {
                let guid = p_obj.get("guid");
                match guid {
                    Some(guid_value) => {
                        if guid_value.is_string() && guid_value.as_str().unwrap() == wt_profile_guid_with_braces {
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
        Some(_profile) => { 
            println!("Profile for `{}` already exists in the Windows Terminal settings.json file. Leaving it as is.", wsl_name);
        },
        None => {
            let profile_object= serde_json::json!(
            {
                "guid": format!("{{{}}}", wt_profile_guid),
                "hidden": false,
                "name": wsl_name,
                "commandline": format!("dragon run -w {}", wsl_name)
            });

            wt_profiles_list_array.insert(0, profile_object);

            write_json_file(windows_terminal_config_path, &wt_config_content)
                .with_context(|| format!("Could not write Windows Terminal settings file `{:#?}`!", windows_terminal_config_path))?;

            println!("Profile for `{}` has been added to the Windows Terminal settings.json file! You can now connect to it in Windows Terminal by opening a new tab!", wsl_name);
        }
    }

    Ok(())
}

fn pull_image_tag(image_url_str: &str) -> Result<()> {
    let mut docker_pull_command = Command::new(r#"docker"#);
    docker_pull_command.args(&["pull", image_url_str]);

    let docker_pull_command_status = docker_pull_command.status()
        .with_context(|| format!("`docker pull {}` failed!", image_url_str))?;
    if !docker_pull_command_status.success() {
        return Err(anyhow::anyhow!("`docker pull {}` failed!", image_url_str));
    }


    Ok(())
}

fn docker_login(registry_name: &str, username: &str, password: &str) -> Result<()> {
    let mut docker_login_command = Command::new(r#"docker"#);
    docker_login_command.args(&["login", registry_name])
                    .args(&["--username", username])
                    .args(&["--password", password]);

    let docker_login_command_status = docker_login_command.status()
        .with_context(|| format!("`docker login {}` failed!", registry_name))?;
    if !docker_login_command_status.success() {
        return Err(anyhow::anyhow!("`docker login {}` failed. Double-check the service principal details in `.dockerwsl`!", registry_name));
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

fn get_dockerwsl_content(file_path: &PathBuf) -> Result<DockerWSLConf> {
    if Path::new(file_path.as_path()).exists() {
        return parse_dockerwslconf_file(file_path);
    } else {
        return Ok(DockerWSLConf {
            wsls: vec![],
            default_base_wsl_install_path: None,
            private_registries: vec![]
        });
    }
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

    let file =  OpenOptions::new().write(true).truncate(true).create(true).open(file_path)
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

    println!("{}-{}-{}", username, password, tenant);

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
        .with_context(|| format!("Failed to retrieve the latest tag for {}/{}!", registry_name, repository_name))?;
    
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

