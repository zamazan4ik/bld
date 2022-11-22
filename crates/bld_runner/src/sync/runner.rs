use crate::platform::TargetPlatform;
use crate::sync::builder::RunnerBuilder;
use crate::sync::pipeline::{BuildStep, External, ExternalDetails, Pipeline};
use actix::{io::SinkWrite, Actor, StreamHandler};
use anyhow::{anyhow, bail, Result};
use bld_config::definitions::{
    ENV_TOKEN, GET, PUSH, RUN_PROPS_ID, RUN_PROPS_START_TIME, VAR_TOKEN,
};
use bld_config::BldConfig;
use bld_core::context::ContextSender;
use bld_core::execution::Execution;
use bld_core::logger::LoggerSender;
use bld_core::proxies::PipelineFileSystemProxy;
use bld_sock::clients::ExecClient;
use bld_sock::messages::{RunInfo, WorkerMessages};
use bld_utils::request::WebSocket;
use bld_utils::sync::IntoArc;
use futures::stream::StreamExt;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::time::sleep;
use tracing::debug;

type RecursiveFuture = Pin<Box<dyn Future<Output = Result<()>>>>;

pub struct Runner {
    pub run_id: String,
    pub run_start_time: String,
    pub cfg: Arc<BldConfig>,
    pub execution: Arc<Execution>,
    pub logger: Arc<LoggerSender>,
    pub prx: Arc<PipelineFileSystemProxy>,
    pub pip: Pipeline,
    pub ipc: Arc<Option<Sender<WorkerMessages>>>,
    pub env: Arc<HashMap<String, String>>,
    pub vars: Arc<HashMap<String, String>>,
    pub context: Arc<ContextSender>,
    pub platform: TargetPlatform,
    pub is_child: bool,
    pub has_faulted: bool,
}

impl Runner {
    async fn register_start(&self) -> Result<()> {
        if !self.is_child {
            debug!("setting the pipeline as running in the execution context");
            self.execution.set_as_running()?;
        }
        Ok(())
    }

    async fn register_completion(&self) -> Result<()> {
        if !self.is_child {
            debug!("setting state of root pipeline");
            if self.has_faulted {
                self.execution.set_as_faulted()?;
            } else {
                self.execution.set_as_finished()?;
            }
        }
        if self.pip.dispose {
            debug!("executing dispose operations for platform");
            self.platform.dispose(self.is_child).await?;
        } else {
            debug!("keeping platform alive");
            self.platform.keep_alive().await?;
        }
        Ok(())
    }

    fn check_stop_signal(&self) -> Result<()> {
        debug!("checking for stop signal");
        self.execution.check_stop_signal()
    }

    async fn ipc_send_completed(&self) -> Result<()> {
        if !self.is_child {
            if let Some(ipc) = Option::as_ref(&self.ipc) {
                debug!("sending message to supervisor for a completed run");
                ipc.send(WorkerMessages::Completed).await?;
            }
        }
        Ok(())
    }

    async fn info(&self) -> Result<()> {
        debug!("printing pipeline informantion");

        if let Some(name) = &self.pip.name {
            let message = format!("Pipeline: {name}");
            self.logger.write_line(message).await?;
        }

        let message = format!("Runs on: {}", self.pip.runs_on);
        self.logger.write_line(message).await?;

        Ok(())
    }

    fn apply_run_properties(&self, txt: &str) -> String {
        let mut txt_with_props = String::from(txt);
        txt_with_props = txt_with_props.replace(RUN_PROPS_ID, &self.run_id);
        txt_with_props = txt_with_props.replace(RUN_PROPS_START_TIME, &self.run_start_time);
        txt_with_props
    }

    fn apply_environment(&self, txt: &str) -> String {
        let mut txt_with_env = String::from(txt);
        for (key, value) in self.env.iter() {
            let full_name = format!("{ENV_TOKEN}{key}");
            txt_with_env = txt_with_env.replace(&full_name, value);
        }
        for env in self.pip.environment.iter() {
            let full_name = format!("{ENV_TOKEN}{}", &env.name);
            txt_with_env = txt_with_env.replace(&full_name, &env.default_value);
        }
        txt_with_env
    }

    fn apply_variables(&self, txt: &str) -> String {
        let mut txt_with_vars = String::from(txt);
        for (key, value) in self.vars.iter() {
            let full_name = format!("{VAR_TOKEN}{key}");
            txt_with_vars = txt_with_vars.replace(&full_name, value);
        }
        for variable in self.pip.variables.iter() {
            let full_name = format!("{VAR_TOKEN}{}", &variable.name);
            txt_with_vars = txt_with_vars.replace(&full_name, &variable.default_value);
        }
        txt_with_vars
    }

    fn apply_context(&self, txt: &str) -> String {
        let txt = self.apply_run_properties(txt);
        let txt = self.apply_environment(&txt);
        self.apply_variables(&txt)
    }

    async fn artifacts(&self, name: &Option<String>) -> Result<()> {
        debug!("executing artifact operation related to step {:?}", name);

        for artifact in self.pip.artifacts.iter().filter(|a| &a.after == name) {
            let can_continue = (artifact.method == Some(PUSH.to_string())
                || artifact.method == Some(GET.to_string()))
                && artifact.from.is_some()
                && artifact.to.is_some();

            if can_continue {
                debug!("applying context for artifact");

                let method = self.apply_context(artifact.method.as_ref().unwrap());
                let from = self.apply_context(artifact.from.as_ref().unwrap());
                let to = self.apply_context(artifact.to.as_ref().unwrap());
                self.logger
                    .write_line(format!(
                        "Copying artifacts from: {from} into container to: {to}",
                    ))
                    .await?;

                let result = match &method[..] {
                    PUSH => {
                        debug!("executing {PUSH} artifact operation");
                        self.platform.push(&from, &to).await
                    }
                    GET => {
                        debug!("executing {GET} artifact operation");
                        self.platform.get(&from, &to).await
                    }
                    _ => unreachable!(),
                };

                if !artifact.ignore_errors {
                    result?;
                }
            }
        }

        Ok(())
    }

    async fn steps(&mut self) -> Result<()> {
        debug!("starting execution of pipeline steps");
        for step in &self.pip.steps {
            self.step(step).await?;
            self.artifacts(&step.name).await?;
            self.check_stop_signal()?;
        }
        Ok(())
    }

    async fn step(&self, step: &BuildStep) -> Result<()> {
        if let Some(name) = &step.name {
            self.logger.write_line(format!("Step: {name}")).await?;
        }
        self.external(step).await?;
        self.sh(step).await?;
        Ok(())
    }

    async fn external(&self, step: &BuildStep) -> Result<()> {
        debug!(
            "starting execution of external section for step {:?}",
            step.name
        );

        for step_external in &step.external {

            let external = self.pip.external.iter().find(|i| match i {
                External::Local(details) => &details.name == step_external,
                External::Server { details, .. } => &details.name == step_external,
            });

            if let Some(external) = external {
                match external {
                    External::Local(details) => self.local_external(details).await?,
                    External::Server { server, details } => self.server_external(server, details).await?,
                };
            }
        }

        Ok(())
    }

    async fn local_external(&self, details: &ExternalDetails) -> Result<()> {
        debug!("building runner for child pipeline");

        let variables: HashMap<String, String> = details
            .variables
            .iter()
            .map(|e| (e.name.to_string(), self.apply_context(&e.default_value)))
            .collect();

        let environment: HashMap<String, String> = details
            .environment
            .iter()
            .map(|e| (e.name.to_string(), self.apply_context(&e.default_value)))
            .collect();

        let runner = RunnerBuilder::default()
            .run_id(&self.run_id)
            .run_start_time(&self.run_start_time)
            .config(self.cfg.clone())
            .proxy(self.prx.clone())
            .pipeline(&details.pipeline)
            .execution(self.execution.clone())
            .logger(self.logger.clone())
            .environment(environment.into_arc())
            .variables(variables.into_arc())
            .ipc(self.ipc.clone())
            .context(self.context.clone())
            .is_child(true)
            .build()
            .await?;

        debug!("starting child pipeline runner");

        runner.run().await.await?;
        self.check_stop_signal()?;

        Ok(())
    }

    async fn server_external(&self, server: &str, details: &ExternalDetails) -> Result<()> {
        let server = self.cfg.remote.server(&server)?;
        let server_auth = self.cfg.remote.same_auth_as(server)?;
        let variables = details
            .variables
            .iter()
            .map(|e| (e.name.to_string(), self.apply_context(&e.default_value)))
            .collect();

        let environment = details
            .environment
            .iter()
            .map(|e| (e.name.to_string(), self.apply_context(&e.default_value)))
            .collect();

        let url = format!(
            "{}://{}:{}/ws-exec/",
            server.ws_protocol(),
            server.host,
            server.port
        );

        debug!(
            "establishing web socket connection with server {}",
            server.name
        );

        let (_, framed) = WebSocket::new(&url)?
            .auth(server_auth)
            .request()
            .connect()
            .await
            .map_err(|e| anyhow!(e.to_string()))?;
        let (sink, stream) = framed.split();
        let addr = ExecClient::create(|ctx| {
            ExecClient::add_stream(stream, ctx);
            ExecClient::new(self.logger.clone(), SinkWrite::new(sink, ctx))
        });

        debug!("sending message for pipeline execution over the web socket");

        addr.send(RunInfo::new(
            &details.pipeline,
            Some(environment),
            Some(variables),
        ))
        .await
        .map_err(|e| anyhow!(e))?;

        while addr.connected() {
            sleep(Duration::from_millis(300)).await;
        }

        Ok(())
    }

    async fn sh(&self, step: &BuildStep) -> Result<()> {
        debug!("start execution of exec section for step");
        for command in step.commands.iter() {
            let working_dir = step.working_dir.as_ref().map(|wd| self.apply_context(wd));
            let command = self.apply_context(command);

            debug!("executing shell command {}", command);
            self.platform
                .shell(&working_dir, &command, self.execution.clone())
                .await?;

            self.check_stop_signal()?;
        }
        Ok(())
    }

    async fn start(&self) -> Result<()> {
        self.register_start().await?;
        self.info().await?;
        Ok(())
    }

    async fn execute(&mut self) -> Result<()> {
        // using let expressions to log the errors and let an empty string be used
        // by the final print_error of main.

        if let Err(e) = self.artifacts(&None).await {
            self.logger.write(e.to_string()).await?;
            self.has_faulted = true;
            bail!("");
        }

        if let Err(e) = self.steps().await {
            self.logger.write(e.to_string()).await?;
            self.has_faulted = true;
            bail!("");
        }

        Ok(())
    }

    async fn cleanup(&self) -> Result<()> {
        debug!("starting cleanup operations for runner");
        self.register_completion().await?;
        self.ipc_send_completed().await?;
        Ok(())
    }

    pub async fn run(mut self) -> RecursiveFuture {
        Box::pin(async move {
            self.start().await?;
            let execution_result = self.execute().await;
            let cleanup_result = self.cleanup().await;
            debug!("runner completed");
            execution_result.and(cleanup_result)
        })
    }
}
