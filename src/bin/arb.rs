extern crate abacom_relay_board;

#[macro_use]
extern crate clap;
extern crate libusb;

use std::io::{self, Write};
use std::process;
use clap::{App, Arg};

type Port = u8;
type Relays = u8;

fn parse_args() -> (bool, Relays, bool, Option<Port>) {
    let matches = App::new("abacom-relay-board")
        .author("Adrian K. <adrian.kumpf@posteo.de>")
        .version(crate_version!())
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .help("Uses a custom port")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("disable-verification")
                .short("d")
                .long("disable-verification")
                .requires("RELAYS")
                .help("Disables the verifaction after activating relays"),
        )
        .arg(
            Arg::with_name("status")
                .short("s")
                .long("status")
                .help("Get relays status")
                .conflicts_with("RELAYS"),
        )
        .arg(
            Arg::with_name("RELAYS")
                .help("Sets the relays to activate")
                .default_value("0")
                .possible_values(&["0", "1", "2", "3", "4", "5", "6", "7", "8"])
                .multiple(true)
                .index(1),
        )
        .get_matches();

    let port = value_t!(matches, "port", u8).ok();
    let status = matches.is_present("status");
    let verify = !matches.is_present("disable-verification");
    let relays = values_t!(matches, "RELAYS", u8)
        .unwrap()
        .iter()
        .filter(|&&r| r != 0)
        .fold(0, |acc, &r| acc | 1 << (r - 1));

    (status, relays, verify, port)
}

fn run() -> abacom_relay_board::Result {
    let (status, relays, verify, port) = parse_args();

    if status {
        let result = abacom_relay_board::get_relays(port)?;

        let active_relays: Vec<_> = (0..8)
            .filter_map(|m| {
                if (1 << m) & result != 0 {
                    Some((m + 1).to_string())
                } else {
                    None
                }
            })
            .collect();

        println!("Active relays: {}", active_relays.join(" "));

        Ok(())
    } else {
        abacom_relay_board::switch_relays(relays, verify, port)
    }
}

fn main() {
    process::exit(match run() {
        Ok(_) => 0,
        Err(err) => {
            writeln!(io::stderr(), "error: {}", err).unwrap();
            1
        }
    });
}
