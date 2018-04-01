extern crate arb;

#[macro_use]
extern crate clap;
extern crate libusb;

use std::process;
use clap::{App, Arg};

struct Args {
    get_status: bool,
    relays: u8,
    verify: bool,
    port: Option<u8>,
}

fn parse_args() -> Args {
    let matches = App::new("abacom-relay-board (arb)")
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
            Arg::with_name("get_status")
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
    let get_status = matches.is_present("get_status");
    let verify = !matches.is_present("disable-verification");
    let relays = values_t!(matches, "RELAYS", u8)
        .unwrap()
        .iter()
        .filter(|&&r| r != 0)
        .fold(0, |acc, &r| acc | 1 << (r - 1));

    Args {
        get_status,
        relays,
        verify,
        port,
    }
}

fn run() -> arb::Result {
    let args = parse_args();

    if args.get_status {
        let result = arb::get_status(args.port)?;

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
        arb::set_status(args.relays, args.verify, args.port)
    }
}

fn main() {
    process::exit(match run() {
        Ok(_) => 0,
        Err(err) => {
            eprintln!("error: {}", err);
            1
        }
    });
}
