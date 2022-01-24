#![feature(proc_macro_hygiene, decl_macro, type_alias_impl_trait)]

#[macro_use]
extern crate rocket;

mod follower;
mod rocket_glue;
mod supervisor;
mod utils;
mod webserver;

use crate::supervisor::SupervisorOptions;
use clap::{App, AppSettings, Arg, SubCommand};

fn main() {
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
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("supervisor", Some(matches)) => {
            supervisor::run(SupervisorOptions {
                server_address: matches.value_of("server-address").unwrap().to_string(),
                gui_port: matches
                    .value_of("gui-port")
                    .unwrap()
                    .parse::<u16>()
                    .unwrap(),
            });
        }
        _ => {
            unreachable!()
        }
    }
}
