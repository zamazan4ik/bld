use crate::BldCommand;
use bld_config::{definitions::TOOL_DEFAULT_PIPELINE, definitions::VERSION, BldConfig};
use bld_core::execution::EmptyExec;
use bld_core::logger::ShellLogger;
use bld_core::proxies::LocalPipelineProxy;
use bld_runner::{self, ExecConnectionInfo, RunnerBuilder};
use bld_utils::errors::auth_for_server_invalid;
use bld_utils::request::headers;
use clap::{App, Arg, ArgMatches, SubCommand};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tracing::debug;
use uuid::Uuid;

static RUN: &str = "run";
static PIPELINE: &str = "pipeline";
static SERVER: &str = "server";
static DETACH: &str = "detach";
static VARIABLES: &str = "variables";

pub struct RunCommand;

impl RunCommand {
    pub fn boxed() -> Box<dyn BldCommand> {
        Box::new(Self)
    }
}

impl BldCommand for RunCommand {
    fn id(&self) -> &'static str {
        RUN
    }

    fn interface(&self) -> App<'static, 'static> {
        let pipeline = Arg::with_name(PIPELINE)
            .short("p")
            .long("pipeline")
            .help("Path to pipeline script")
            .takes_value(true);
        let server = Arg::with_name(SERVER)
            .short("s")
            .long("server")
            .help("The name of the server to run the pipeline")
            .takes_value(true);
        let detach = Arg::with_name(DETACH)
            .short("d")
            .long("detach")
            .help("Detaches from the run execution (for server mode runs)");
        let variables = Arg::with_name(VARIABLES)
            .short("v")
            .long("variables")
            .help("Define values for variables of a pipeline")
            .multiple(true)
            .takes_value(true);
        SubCommand::with_name(RUN)
            .about("Executes a build pipeline")
            .version(VERSION)
            .args(&[pipeline, server, detach, variables])
    }

    fn exec(&self, matches: &ArgMatches<'_>) -> anyhow::Result<()> {
        let config = BldConfig::load()?;
        let pipeline = matches
            .value_of("pipeline")
            .or(Some(TOOL_DEFAULT_PIPELINE))
            .unwrap()
            .to_string();
        let detach = matches.is_present("detach");
        let vars = matches
            .values_of("variables")
            .map(|variable| {
                variable
                    .map(|v| {
                        let mut split = v.split('=');
                        let name = split.next().or(Some("")).unwrap().to_string();
                        let value = split.next().or(Some("")).unwrap().to_string();
                        (name, value)
                    })
                    .collect::<HashMap<String, String>>()
            })
            .or_else(|| Some(HashMap::new()))
            .unwrap();
        match matches.value_of("server") {
            Some(server) => {
                let srv = config.remote.server(server)?;
                let (srv_name, auth) = match &srv.same_auth_as {
                    Some(name) => match config.remote.servers.iter().find(|s| &s.name == name) {
                        Some(srv) => (&srv.name, &srv.auth),
                        None => return auth_for_server_invalid(),
                    },
                    None => (&srv.name, &srv.auth),
                };
                debug!(
                    "running {} subcommand with --pipeline: {}, --variables: {:?}, --server: {}",
                    RUN,
                    pipeline,
                    vars,
                    server.to_string()
                );
                bld_runner::on_server(ExecConnectionInfo {
                    host: srv.host.clone(),
                    port: srv.port,
                    headers: headers(srv_name, auth)?,
                    detach,
                    pipeline,
                    variables: vars,
                })
            }
            None => {
                debug!(
                    "running {} subcommand with --pipeline: {}, --variables: {:?}",
                    RUN, pipeline, vars
                );
                let id = Uuid::new_v4().to_string();
                let start_time = chrono::offset::Local::now().format("%F %X").to_string();
                let rt = Runtime::new()?;
                rt.block_on(async {
                    let runner = RunnerBuilder::default()
                        .set_run_id(&id)
                        .set_run_start_time(&start_time)
                        .set_config(Arc::new(config))
                        .set_proxy(Arc::new(LocalPipelineProxy))
                        .set_pipeline(&pipeline)?
                        .set_exec(EmptyExec::atom())
                        .set_log(ShellLogger::atom())
                        .set_variables(Arc::new(vars))
                        .build()
                        .await?;
                    runner.run().await.await
                })
            }
        }
    }
}