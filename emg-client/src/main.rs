#![feature(
    type_alias_impl_trait,
    inline_const,
    lazy_cell,
    never_type,
    array_methods
)]

#[cfg(feature = "bluetooth")]
mod bluetooth;
mod follower;
mod remote_time_estimator;
#[cfg(not(feature = "bluetooth"))]
mod serial_port_communication;
mod signal;
mod supervisor;
mod utils;
mod webserver;
mod webserver_glue;

use crate::follower::LocalFollower;
use crate::supervisor::{Supervisor, SupervisorOptions};
use clap::{App, AppSettings, Arg, SubCommand};
use rodio::OutputStream;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let matches = App::new("EliDupree's EMG Mouse Client")
        .version("0.1")
        .author("Eli Dupree <vcs@elidupree.com>")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("supervisor")
                .long_about("Listens for EMG input and does stuff with it")
                .arg(
                    Arg::with_name("server-address")
                        .long("server-address")
                        .required(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("gui-port")
                        .long("gui-port")
                        .required(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("follower-port")
                        .long("follower-port")
                        .required(true)
                        .takes_value(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("follower")
                .long_about("Listens for instructions from supervisor")
                .arg(
                    Arg::with_name("supervisor-address")
                        .long("supervisor-address")
                        .required(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("supervisor-cert-path")
                        .long("supervisor-cert-path")
                        .required(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("name")
                        .long("name")
                        .required(true)
                        .takes_value(true),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("supervisor", Some(matches)) => {
            Supervisor::run(SupervisorOptions {
                server_address: matches.value_of("server-address").unwrap().to_string(),
                gui_port: matches
                    .value_of("gui-port")
                    .unwrap()
                    .parse::<u16>()
                    .unwrap(),
                follower_port: matches
                    .value_of("follower-port")
                    .unwrap()
                    .parse::<u16>()
                    .unwrap(),
            })
            .await
        }
        ("follower", Some(matches)) => {
            let (_audio_output_stream, audio_output_stream_handle) =
                OutputStream::try_default().unwrap();
            LocalFollower::new(audio_output_stream_handle)
                .listen_to_remote(
                    matches.value_of("supervisor-address").unwrap(),
                    matches.value_of("supervisor-cert-path").unwrap(),
                    matches.value_of("name").unwrap().to_string(),
                )
                .await?
        }
        _ => {
            unreachable!()
        }
    }
}
