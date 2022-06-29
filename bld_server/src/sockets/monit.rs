use crate::extractors::User;
use crate::requests::MonitInfo;
use actix::prelude::*;
use actix_web::{error::ErrorUnauthorized, web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use anyhow::anyhow;
use bld_config::{path, BldConfig};
use bld_core::database::pipeline_runs::{self, PipelineRuns};
use bld_core::scanner::{FileScanner, Scanner};
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::sqlite::SqliteConnection;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct MonitorPipelineSocket {
    hb: Instant,
    id: String,
    pool: web::Data<Pool<ConnectionManager<SqliteConnection>>>,
    config: web::Data<BldConfig>,
    scanner: Option<FileScanner>,
}

impl MonitorPipelineSocket {
    pub fn new(
        pool: web::Data<Pool<ConnectionManager<SqliteConnection>>>,
        config: web::Data<BldConfig>,
    ) -> Self {
        Self {
            hb: Instant::now(),
            id: String::new(),
            pool,
            config,
            scanner: None,
        }
    }

    fn heartbeat(act: &Self, ctx: &mut <Self as Actor>::Context) {
        if Instant::now().duration_since(act.hb) > Duration::from_secs(10) {
            println!("Websocket heartbeat failed, disconnecting!");
            ctx.stop();
            return;
        }
        ctx.ping(b"");
    }

    fn scan(act: &mut Self, ctx: &mut <Self as Actor>::Context) {
        if let Some(scanner) = act.scanner.as_mut() {
            let content = scanner.fetch();
            for line in content.iter() {
                ctx.text(line.to_string());
            }
        }
    }

    fn exec(act: &mut Self, ctx: &mut <Self as Actor>::Context) {
        if let Ok(connection) = act.pool.get() {
            match pipeline_runs::select_by_id(&connection, &act.id) {
                Ok(PipelineRuns { running: false, .. }) => ctx.stop(),
                Err(_) => {
                    ctx.text("internal server error");
                    ctx.stop();
                }
                _ => {}
            }
        }
    }

    fn dependencies(&mut self, data: &str) -> anyhow::Result<()> {
        let data = serde_json::from_str::<MonitInfo>(data)?;
        let conn = self.pool.get()?;

        let run = if data.last {
            pipeline_runs::select_last(&conn)
        } else if let Some(id) = data.id {
            pipeline_runs::select_by_id(&conn, &id)
        } else if let Some(name) = data.name {
            pipeline_runs::select_by_name(&conn, &name)
        } else {
            return Err(anyhow!("pipeline not found"));
        }
        .map_err(|_| anyhow!("pipeline not found"))?;

        self.id = run.id.clone();

        self.scanner = Some(FileScanner::new(Arc::clone(&self.config), &run.id));
        Ok(())
    }
}

impl Actor for MonitorPipelineSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(Duration::from_millis(500), |act, ctx| {
            MonitorPipelineSocket::heartbeat(act, ctx);
            MonitorPipelineSocket::scan(act, ctx);
        });
        ctx.run_interval(Duration::from_secs(1), |act, ctx| {
            MonitorPipelineSocket::exec(act, ctx);
        });
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MonitorPipelineSocket {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Text(txt)) => {
                if let Err(e) = self.dependencies(&txt) {
                    eprintln!("{e}");
                    ctx.text("internal server error");
                    ctx.stop();
                }
            }
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}

pub async fn ws_monit(
    user: Option<User>,
    req: HttpRequest,
    stream: web::Payload,
    pool: web::Data<Pool<ConnectionManager<SqliteConnection>>>,
    config: web::Data<BldConfig>,
) -> Result<HttpResponse, Error> {
    if user.is_none() {
        return Err(ErrorUnauthorized(""));
    }
    println!("{req:?}");
    let res = ws::start(MonitorPipelineSocket::new(pool, config), &req, stream);
    println!("{res:?}");
    res
}
