use crate::command::BldCommand;
use crate::run::parse_variables;
use actix::io::SinkWrite;
use actix::{Actor, StreamHandler};
use actix_web::rt::{spawn, System};
use anyhow::{anyhow, Result};
use bld_config::BldConfig;
use bld_core::context::ContextSender;
use bld_core::database::{new_connection_pool, pipeline_runs};
use bld_core::execution::Execution;
use bld_core::logger::LoggerSender;
use bld_core::proxies::PipelineFileSystemProxy;
use bld_runner::RunnerBuilder;
use bld_sock::clients::WorkerClient;
use bld_sock::messages::WorkerMessages;
use bld_utils::request::WebSocket;
use bld_utils::sync::IntoArc;
use clap::Args;
use futures::join;
use futures::stream::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc::{channel, Receiver};
use tracing::{debug, error};

#[derive(Args)]
#[command(
    about = "A sub command that creates a worker process for a bld server in order to run a pipeline."
)]
pub struct WorkerCommand {
    #[arg(
        short = 'p',
        long = "pipeline",
        required = true,
        help = "The pipeline id in the current bld server instance"
    )]
    pipeline: String,

    #[arg(
        short = 'r',
        long = "run-id",
        required = true,
        help = "The target pipeline run id"
    )]
    run_id: String,

    #[arg(
        short = 'v',
        long = "variable",
        help = "Define value for a variable in the server pipeline"
    )]
    variables: Vec<String>,

    #[arg(
        short = 'e',
        long = "environment",
        help = "Define values for environment variables in the server pipeline"
    )]
    environment: Vec<String>,
}

impl BldCommand for WorkerCommand {
    fn exec(self) -> Result<()> {
        let config = BldConfig::load()?.into_arc();
        let socket_cfg = config.clone();

        let pipeline = self.pipeline.into_arc();
        let run_id = self.run_id.into_arc();
        let variables = parse_variables(&self.variables).into_arc();
        let environment = parse_variables(&self.environment).into_arc();

        let pool = new_connection_pool(&config.local.db)?.into_arc();
        let mut conn = pool.get()?;
        let pipeline_run = pipeline_runs::select_by_id(&mut conn, &run_id)?;
        let start_date_time = pipeline_run.start_date_time;
        let proxy = PipelineFileSystemProxy::Server {
            config: config.clone(),
            pool: pool.clone(),
        }
        .into_arc();

        let exec = Execution::new(pool.clone(), &run_id).into_arc();

        let (worker_tx, worker_rx) = channel(4096);
        let worker_tx = Some(worker_tx).into_arc();

        System::new().block_on(async move {
            let logger = LoggerSender::file(config.clone(), &run_id)?.into_arc();
            let context = ContextSender::new(pool, &run_id).into_arc();

            let socket_handle = spawn(async move {
                if let Err(e) = connect_to_supervisor(socket_cfg, worker_rx).await {
                    error!("{e}");
                }
            });

            let runner_handle = spawn(async move {
                match RunnerBuilder::default()
                    .run_id(&run_id)
                    .run_start_time(&start_date_time)
                    .config(config)
                    .proxy(proxy)
                    .pipeline(&pipeline)
                    .execution(exec)
                    .logger(logger)
                    .environment(environment)
                    .variables(variables)
                    .context(context)
                    .ipc(worker_tx)
                    .build()
                    .await
                {
                    Ok(runner) => {
                        if let Err(e) = runner.run().await.await {
                            error!("error with runner, {e}");
                        }
                    }
                    Err(e) => error!("failed on building the runner, {e}"),
                }
            });

            let _ = join!(socket_handle, runner_handle);

            Ok(())
        })
    }
}

async fn connect_to_supervisor(
    config: Arc<BldConfig>,
    mut worker_rx: Receiver<WorkerMessages>,
) -> Result<()> {
    let protocol = config.local.supervisor.ws_protocol();
    let url = format!(
        "{protocol}://{}:{}/ws-worker/",
        config.local.supervisor.host, config.local.supervisor.port
    );

    debug!("establishing web socket connection on {}", url);

    let (_, framed) = WebSocket::new(&url)?
        .request()
        .connect()
        .await
        .map_err(|e| {
            error!("{e}");
            anyhow!(e.to_string())
        })?;

    let (sink, stream) = framed.split();
    let addr = WorkerClient::create(|ctx| {
        WorkerClient::add_stream(stream, ctx);
        WorkerClient::new(SinkWrite::new(sink, ctx))
    });

    addr.send(WorkerMessages::Ack).await?;
    addr.send(WorkerMessages::WhoAmI {
        pid: std::process::id(),
    })
    .await?;

    while let Some(msg) = worker_rx.recv().await {
        debug!("sending message to supervisor {:?}", msg);
        addr.send(msg).await?
    }

    Ok(())
}
