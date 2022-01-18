mod supervisor;

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
                ),
        )
        .get_matches();

    match matches.subcommand() {
        ("supervisor", Some(matches)) => {
            supervisor::run(SupervisorOptions {
                server_address: matches.value_of("server-address").unwrap().to_string(),
            });
        }
        _ => {
            unreachable!()
        }
    }
}
