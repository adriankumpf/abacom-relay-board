extern crate abacom_relay_board;

#[macro_use]
extern crate clap;
extern crate libusb;

use std::io::{self, Write};
use std::process;
use clap::{App, Arg, ArgMatches};

fn parse_args<'a>() -> ArgMatches<'a> {
    App::new("abacom-relay-board")
        .author("Adrian K. <adrian.kumpf@posteo.de>")
        .version(crate_version!())
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .help("Uses a custom port")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("RELAYS")
                .help("Sets the relays to activate")
                .required(true)
                .max_values(8)
                .possible_values(&["0", "1", "2", "3", "4", "5", "6", "7", "8"])
                .multiple(true)
                .index(1),
        )
        .get_matches()
}

fn run_app() -> abacom_relay_board::Result<()> {
    let matches = parse_args();

    let port = value_t!(matches, "port", u8).ok();
    let relays = values_t!(matches, "RELAYS", u8).unwrap();

    abacom_relay_board::switch_relays(relays, port)
}

fn main() {
    process::exit(match run_app() {
        Ok(_) => 0,
        Err(err) => {
            writeln!(io::stderr(), "error: {}", err).unwrap();
            1
        }
    });
}
