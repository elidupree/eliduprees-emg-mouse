use crate::supervisor::NewFrontendSession;
use crate::webserver::{MessageFromFrontend, MessageToFrontend};
use crate::Supervisor;
use actix::{Actor, Addr, AsyncContext, Handler, StreamHandler};
use actix_files::NamedFile;
use actix_web::{get, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use std::path::PathBuf;

struct WebserverState {
    supervisor: Addr<Supervisor>,
    static_files: PathBuf,
}

pub struct FrontendSession {
    supervisor: Addr<Supervisor>,
}

impl Actor for FrontendSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, context: &mut Self::Context) {
        self.supervisor.do_send(NewFrontendSession {
            session: context.address(),
        });
    }
}

impl Handler<MessageToFrontend> for FrontendSession {
    type Result = ();

    fn handle(&mut self, message: MessageToFrontend, context: &mut Self::Context) -> Self::Result {
        context.text(serde_json::to_string(&message).unwrap());
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for FrontendSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => ctx.pong(&msg),
            Ok(ws::Message::Text(text)) => {
                println!("Received from frontend: {}", text);
                let message = serde_json::from_str::<MessageFromFrontend>(&text);
                println!("Deserialized: {:?}", message);
                if let Ok(message) = message {
                    self.supervisor.do_send(message)
                }
            }
            _ => (),
        }
    }
}

#[get("/session")]
async fn session(
    req: HttpRequest,
    stream: web::Payload,
    webserver_state: web::Data<WebserverState>,
) -> Result<HttpResponse, Error> {
    ws::start(
        FrontendSession {
            supervisor: webserver_state.supervisor.clone(),
        },
        &req,
        stream,
    )
}

#[get("/")]
async fn index(webserver_state: web::Data<WebserverState>) -> Option<NamedFile> {
    NamedFile::open(webserver_state.static_files.join("index2.html")).ok()
}

pub async fn launch(
    supervisor: Addr<Supervisor>,
    static_files: PathBuf,
    port: u16,
) -> anyhow::Result<()> {
    let state = web::Data::new(WebserverState {
        supervisor,
        static_files,
    });
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .service(index)
            .service(actix_files::Files::new("/media", "./web_frontend/media"))
            .service(session)
    })
    .workers(1)
    .bind(("localhost", port))
    .unwrap()
    .run()
    .await?;
    Ok(())
}
