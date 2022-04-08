use crate::webserver::{FrontendState, MessageFromFrontend};
use crate::Supervisor;
use actix::Addr;
use actix_files::NamedFile;
use actix_web::{get, post, web, App, HttpServer, Responder};
use crossbeam::atomic::AtomicCell;
use std::path::PathBuf;
use std::sync::Arc;

struct WebserverState {
    supervisor: Addr<Supervisor>,
    state_updater: Arc<AtomicCell<Option<FrontendState>>>,
    static_files: PathBuf,
}

#[post("/state_update")]
async fn state_update(webserver_state: web::Data<WebserverState>) -> impl Responder {
    web::Json(webserver_state.state_updater.take())
}

#[post("/input")]
async fn input(
    webserver_state: web::Data<WebserverState>,
    input: web::Json<MessageFromFrontend>,
) -> &'static str {
    let web::Json(input) = input;
    webserver_state.supervisor.do_send(input);
    ""
}

#[get("/")]
async fn index(webserver_state: web::Data<WebserverState>) -> Option<NamedFile> {
    NamedFile::open(webserver_state.static_files.join("index.html")).ok()
}

pub async fn launch(
    state_updater: Arc<AtomicCell<Option<FrontendState>>>,
    supervisor: Addr<Supervisor>,
    static_files: PathBuf,
    port: u16,
) -> anyhow::Result<()> {
    let state = web::Data::new(WebserverState {
        state_updater,
        supervisor,
        static_files,
    });
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .service(index)
            .service(state_update)
            .service(input)
    })
    .workers(1)
    .bind(("localhost", port))
    .unwrap()
    .run()
    .await?;
    Ok(())
}
