use crate::BldCommand;
use actix_web::{middleware, rt::System, web, App, HttpServer};
use bld_config::{definitions::VERSION, BldConfig};
use bld_core::database::new_connection_pool;
use bld_core::high_avail::HighAvail;
use bld_core::proxies::ServerPipelineProxy;
use bld_server::endpoints::{
    auth_redirect, deps, ha_append_entries, ha_install_snapshot, ha_vote, hist, home, inspect,
    list, pull, push, remove, stop,
};
use bld_server::sockets::{ws_exec, ws_high_avail, ws_monit};
use bld_server::state::PipelinePool;
use clap::{App as ClapApp, Arg, ArgMatches, SubCommand};
use std::env::set_var;
use std::sync::Arc;
use tracing::{debug, info};

static SERVER: &str = "server";
static HOST: &str = "host";
static PORT: &str = "port";

pub struct ServerCommand;

impl ServerCommand {
    pub fn boxed() -> Box<dyn BldCommand> {
        Box::new(Self)
    }

    async fn start(config: BldConfig, host: &str, port: i64) -> anyhow::Result<()> {
        info!("starting bld server at {}:{}", host, port);
        let db_pool = new_connection_pool(&config.local.db)?;
        let pip_pool = web::Data::new(PipelinePool::default());
        let ha = web::Data::new(HighAvail::new(&config, db_pool.clone()).await?);
        let db_pool = web::Data::new(db_pool);
        let cfg = web::Data::new(config);
        let prx = web::Data::new(ServerPipelineProxy::new(
            Arc::clone(&cfg),
            Arc::clone(&db_pool),
        ));
        set_var("RUST_LOG", "actix_server=info,actix_web=debug");
        HttpServer::new(move || {
            App::new()
                .app_data(pip_pool.clone())
                .app_data(cfg.clone())
                .app_data(ha.clone())
                .app_data(db_pool.clone())
                .app_data(prx.clone())
                .wrap(middleware::Logger::default())
                .service(ha_append_entries)
                .service(ha_install_snapshot)
                .service(ha_vote)
                .service(home)
                .service(auth_redirect)
                .service(hist)
                .service(list)
                .service(remove)
                .service(push)
                .service(deps)
                .service(pull)
                .service(stop)
                .service(inspect)
                .service(web::resource("/ws-exec/").route(web::get().to(ws_exec)))
                .service(web::resource("/ws-monit/").route(web::get().to(ws_monit)))
                .service(web::resource("/ws-ha/").route(web::get().to(ws_high_avail)))
        })
        .bind(format!("{host}:{port}"))?
        .run()
        .await?;
        Ok(())
    }

    pub fn spawn(config: BldConfig, host: String, port: i64) -> anyhow::Result<()> {
        debug!("starting actix system");
        System::new().block_on(async move {
            let _ = Self::start(config, &host, port).await;
        });
        Ok(())
    }
}

impl BldCommand for ServerCommand {
    fn id(&self) -> &'static str {
        SERVER
    }

    fn interface(&self) -> ClapApp<'static, 'static> {
        let host = Arg::with_name(HOST)
            .long("host")
            .short("H")
            .help("The server's host address")
            .takes_value(true);
        let port = Arg::with_name(PORT)
            .long("port")
            .short("P")
            .help("The server's port")
            .takes_value(true);
        SubCommand::with_name(SERVER)
            .about("Start bld in server mode, listening to incoming build requests")
            .version(VERSION)
            .args(&[host, port])
    }

    fn exec(&self, matches: &ArgMatches<'_>) -> anyhow::Result<()> {
        let config = BldConfig::load()?;
        let host = matches
            .value_of("host")
            .or(Some(&config.local.host))
            .unwrap()
            .to_string();
        let port = matches
            .value_of("port")
            .map(|port| port.parse::<i64>().unwrap_or(config.local.port))
            .unwrap_or(config.local.port);
        debug!("running {SERVER} subcommand with --host: {host} --port: {port}",);
        Self::spawn(config, host, port)?;
        Ok(())
    }
}