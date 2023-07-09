use crate::command::BldCommand;
use actix_web::rt::System;
use anyhow::Result;
use bld_config::BldConfig;
use bld_core::{proxies::PipelineFileSystemProxy, request::HttpClient};
use bld_utils::sync::IntoArc;
use clap::Args;

#[derive(Args)]
#[command(about = "Print the contents of a pipeline")]
pub struct CatCommand {
    #[arg(long = "verbose", help = "Sets the level of verbosity")]
    verbose: bool,

    #[arg(
        short = 'p',
        long = "pipeline",
        required = true,
        help = "The name of the pipeline to print"
    )]
    pipeline: String,

    #[arg(
        short = 's',
        long = "server",
        help = "The name of the server to print the pipeline from"
    )]
    server: Option<String>,
}

impl CatCommand {
    fn local_print(&self) -> Result<()> {
        let config = BldConfig::load()?.into_arc();
        let proxy = PipelineFileSystemProxy::local(config);
        let pipeline = proxy.read(&self.pipeline)?;
        println!("{pipeline}");
        Ok(())
    }

    fn remote_print(&self, server: &str) -> Result<()> {
        System::new().block_on(async move {
            let config = BldConfig::load()?.into_arc();
            HttpClient::new(config, server)
                .print(&self.pipeline)
                .await
                .map(|r| println!("{r}"))
        })
    }
}

impl BldCommand for CatCommand {
    fn verbose(&self) -> bool {
        self.verbose
    }

    fn exec(self) -> Result<()> {
        match &self.server {
            Some(srv) => self.remote_print(srv),
            None => self.local_print(),
        }
    }
}