extern crate abacom_relay_board;

#[macro_use]
extern crate clap;
extern crate libusb;

use std::io::{self, Write};
use std::process;
use clap::{App, Arg};

type Port = u8;
type Relays = u8;
// type Relays = Vec<u8>;

fn parse_args<'a>() -> (Option<Port>, Relays) {
    let matches = App::new("abacom-relay-board")
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
                // .possible_values(&["0", "1", "2", "3", "4", "5", "6", "7", "8"])
                // .multiple(true)
                .multiple(false)
                .index(1),
        )
        .get_matches();

    let port = value_t!(matches, "port", u8).ok();
    let relays = value_t!(matches, "RELAYS", u8).unwrap();

    // let mut relays = values_t!(matches, "RELAYS", u8).unwrap();
    // relays.dedup();

    (port, relays)
}

fn run_app() -> abacom_relay_board::Result {
    let (port, relays) = parse_args();
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
