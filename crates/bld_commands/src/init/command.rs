use crate::command::BldCommand;
use anyhow::{bail, Result};
use bld_config::definitions;
use bld_config::path;
use bld_utils::term::print_info;
use clap::Args;
use std::fs;
use std::path::Component::Normal;
use std::path::{Path, PathBuf};
use tracing::debug;

#[derive(Args)]
#[command(about = "Initializes the build configuration")]
pub struct InitCommand {
    #[arg(
        short = 's',
        long = "server",
        help = "Initialize configuration for a bld server"
    )]
    is_server: bool,
}

impl BldCommand for InitCommand {
    fn exec(self) -> Result<()> {
        let build_dir_exists = build_dir_exists()?;
        if !build_dir_exists {
            debug!("running init subcommand with --server: {}", self.is_server);
            return create_build_dir()
                .and_then(|_| create_logs_dir(self.is_server))
                .and_then(|_| create_db_dir(self.is_server))
                .and_then(|_| create_server_pipelines_dir(self.is_server))
                .and_then(|_| create_default_yaml())
                .and_then(|_| create_config_yaml(self.is_server));
        }
        let message = format!(
            "{} dir already exists in the current directory",
            definitions::TOOL_DIR
        );
        bail!(message)
    }
}

fn print_dir_created(dir: &str) -> Result<()> {
    print_info(&format!("{} directory created", dir))
}

fn build_dir_exists() -> Result<bool> {
    let curr_dir = std::env::current_dir()?;
    for entry in fs::read_dir(&curr_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let component = path.components().last();
            if let Some(Normal(name)) = component {
                if name == definitions::TOOL_DIR {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn create_build_dir() -> Result<()> {
    let path = Path::new(definitions::TOOL_DIR);
    fs::create_dir(path)?;
    print_dir_created(definitions::TOOL_DIR)?;
    Ok(())
}

fn create_logs_dir(is_server: bool) -> Result<()> {
    if is_server {
        let path = Path::new(definitions::LOCAL_LOGS);
        fs::create_dir(path)?;
        print_dir_created(definitions::LOCAL_LOGS)?;
    }
    Ok(())
}

fn create_db_dir(is_server: bool) -> Result<()> {
    if is_server {
        let path = Path::new(definitions::LOCAL_DB);
        fs::create_dir(path)?;
        print_dir_created(definitions::LOCAL_DB)?;
    }
    Ok(())
}

fn create_server_pipelines_dir(is_server: bool) -> Result<()> {
    if is_server {
        let path = Path::new(definitions::LOCAL_SERVER_PIPELINES);
        fs::create_dir(path)?;
        print_dir_created(definitions::LOCAL_SERVER_PIPELINES)?;
    }
    Ok(())
}

fn create_default_yaml() -> Result<()> {
    let path = path![
        definitions::TOOL_DIR,
        definitions::TOOL_DEFAULT_PIPELINE_FILE
    ];
    fs::write(path, definitions::DEFAULT_PIPELINE_CONTENT)?;
    print_info(&format!(
        "{} yaml file created",
        definitions::TOOL_DEFAULT_PIPELINE
    ))?;
    Ok(())
}

fn create_config_yaml(is_server: bool) -> Result<()> {
    let path = path![definitions::TOOL_DIR, definitions::TOOL_DEFAULT_CONFIG_FILE];
    let content = match is_server {
        true => definitions::default_server_config(),
        false => definitions::default_client_config(),
    };
    fs::write(path, &content)?;
    print_info("config file created")?;
    Ok(())
}
