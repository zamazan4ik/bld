use actix::System;
use anyhow::Result;
use bld_config::definitions::DEFAULT_V2_PIPELINE_CONTENT;
use bld_config::BldConfig;
use bld_core::proxies::PipelineFileSystemProxy;
use bld_core::request::HttpClient;
use bld_utils::sync::IntoArc;
use clap::Args;
use tracing::debug;
use uuid::Uuid;

use crate::command::BldCommand;

#[derive(Args)]
#[command(about = "Creates a new pipeline")]
pub struct AddCommand {
    #[arg(long = "verbose", help = "Sets the level of verbosity")]
    verbose: bool,

    #[arg(
        short = 'p',
        long = "pipeline",
        help = "The path to the new pipeline file"
    )]
    pipeline: String,

    #[arg(
        short = 's',
        long = "server",
        help = "The name of the server to add the pipeline to"
    )]
    server: Option<String>,

    #[arg(
        short = 'e',
        long = "edit",
        help = "Edit the pipeline file immediatelly after creation"
    )]
    edit: bool,
}

impl AddCommand {
    fn local_add(&self) -> Result<()> {
        let config = BldConfig::load()?.into_arc();
        let proxy = PipelineFileSystemProxy::local(config);

        proxy.create(&self.pipeline, DEFAULT_V2_PIPELINE_CONTENT, false)?;

        if self.edit {
            proxy.edit(&self.pipeline)?;
        }

        Ok(())
    }

    fn remote_add(&self, server: &str) -> Result<()> {
        System::new().block_on(async move {
            let config = BldConfig::load()?.into_arc();
            let proxy = PipelineFileSystemProxy::local(config.clone());
            let client = HttpClient::new(config, server);
            let tmp_name = format!("{}.yaml", Uuid::new_v4());

            println!("Creating temporary local pipeline {}", tmp_name);
            debug!("creating temporary pipeline file: {tmp_name}");
            proxy.create_tmp(&tmp_name, DEFAULT_V2_PIPELINE_CONTENT, true)?;

            if self.edit {
                println!("Editing temporary local pipeline {}", tmp_name);
                debug!("starting editor for temporary pipeline file: {tmp_name}");
                proxy.edit_tmp(&tmp_name)?;
            }

            debug!("reading content of temporary pipeline file: {tmp_name}");
            let tmp_content = proxy.read_tmp(&tmp_name)?;

            println!("Pushing updated content for {}", self.pipeline);

            client.push(&self.pipeline, &tmp_content).await?;

            debug!("deleting temporary pipeline file: {tmp_name}");
            proxy.remove_tmp(&tmp_name)?;

            Ok(())
        })
    }
}

impl BldCommand for AddCommand {
    fn verbose(&self) -> bool {
        self.verbose
    }

    fn exec(self) -> Result<()> {
        debug!(
            "running add subcommand with --server: {:?}, --pipeline: {} and --edit: {}",
            self.server, self.pipeline, self.edit
        );

        match &self.server {
            Some(srv) => self.remote_add(srv),
            None => self.local_add(),
        }
    }
}