use crate::supervisor::MessageToSupervisor;
use crate::webserver::{FrontendState, MessageFromFrontend};
use crossbeam::atomic::AtomicCell;
use rocket::config::{Environment, LoggingLevel};
use rocket::response::NamedFile;
use rocket::{Config, State};
use rocket_contrib::json::Json;
use rocket_contrib::serve::StaticFiles;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

struct RocketState {
    sender: Mutex<Sender<MessageToSupervisor>>,
    state_updater: Arc<AtomicCell<Option<FrontendState>>>,
    static_files: PathBuf,
}

#[post("/state_update")]
fn state_update(rocket_state: State<RocketState>) -> Json<Option<FrontendState>> {
    Json(rocket_state.state_updater.take())
}

#[allow(clippy::unit_arg)]
// why is this needed? no idea, probably rocket proc macro stuff
#[post("/input", data = "<input>")]
fn input(input: Json<MessageFromFrontend>, rocket_state: State<RocketState>) {
    let Json(input) = input;

    rocket_state
        .sender
        .lock()
        .unwrap()
        .send(MessageToSupervisor::FromFrontend(input))
        .unwrap();
}

#[get("/")]
fn index(rocket_state: State<RocketState>) -> Option<NamedFile> {
    NamedFile::open(rocket_state.static_files.join("index.html")).ok()
}

pub fn launch(
    state_updater: Arc<AtomicCell<Option<FrontendState>>>,
    sender: Sender<MessageToSupervisor>,
    static_files: PathBuf,
    port: u16,
) {
    rocket::custom(
        Config::build(Environment::Development)
            .address("localhost")
            .port(port)
            .log_level(LoggingLevel::Off)
            .unwrap(),
    )
    //.mount("/media/", StaticFiles::from("../media"))
    .mount("/", routes![index, state_update, input])
    .manage(RocketState {
        state_updater,
        sender: Mutex::new(sender),
        static_files,
    })
    .launch();
}
