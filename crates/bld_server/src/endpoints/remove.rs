use crate::extractors::User;
use actix_web::web::{Data, Json};
use actix_web::{post, HttpResponse};
use bld_core::proxies::PipelineFileSystemProxy;
use tracing::info;

#[post("/remove")]
pub async fn remove(
    _: User,
    prx: Data<PipelineFileSystemProxy>,
    body: Json<String>,
) -> HttpResponse {
    info!("Reached handler for /remove route");
    match prx.remove(&body.into_inner()) {
        Ok(_) => HttpResponse::Ok().json(""),
        Err(e) => HttpResponse::BadRequest().body(e.to_string()),
    }
}
