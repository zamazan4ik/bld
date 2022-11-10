use crate::queues::WorkerQueue;
use crate::sockets::{ws_server_socket, ws_worker_socket};
use actix_web::web::{get, resource, Data};
use actix_web::{App, HttpServer};
use anyhow::{anyhow, Result};
use bld_config::BldConfig;
use bld_core::database::new_connection_pool;
use bld_utils::tls::{load_server_certificate, load_server_private_key};
use rustls::ServerConfig;
use std::sync::Mutex;

pub async fn start(config: BldConfig) -> Result<()> {
    let address = format!(
        "{}:{}",
        config.local.supervisor.host, config.local.supervisor.port
    );
    let config = Data::new(config);
    let config_clone = config.clone();
    let pool = Data::new(new_connection_pool(&config.local.db)?);
    let worker_queue = Data::new(Mutex::new(WorkerQueue::new(
        config.local.supervisor.workers.try_into()?,
        config.clone(),
        pool.clone(),
    )));

    let mut server = HttpServer::new(move || {
        App::new()
            .app_data(config_clone.clone())
            .app_data(pool.clone())
            .app_data(worker_queue.clone())
            .service(resource("/ws-server/").route(get().to(ws_server_socket)))
            .service(resource("/ws-worker/").route(get().to(ws_worker_socket)))
    });

    server = match &config.local.supervisor.tls {
        Some(tls) => {
            let cert_chain = load_server_certificate(&tls.cert_chain)?;
            let private_key = load_server_private_key(&tls.private_key)?;
            let builder = ServerConfig::builder()
                .with_safe_defaults()
                .with_no_client_auth()
                .with_single_cert(cert_chain, private_key)?;
            server.bind_rustls(address, builder)?
        }
        None => server.bind(address)?,
    };

    server.run().await.map_err(|e| anyhow!(e))
}