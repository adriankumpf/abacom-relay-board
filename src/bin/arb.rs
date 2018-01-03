extern crate abacom_relay_board;

#[macro_use]
extern crate clap;
extern crate libusb;

use clap::{App, Arg};

fn main() {
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
                .max_values(8)
                .multiple(true)
                .index(1),
        )
        .get_matches();

    if let Some(p) = matches.value_of("port") {
        println!("Value for port: {}", p);
    }

    let relays: Vec<_> = matches.values_of("RELAYS").unwrap().collect();
    println!("Using relays: {:?}", relays);

    for rb in abacom_relay_board::list_relay_boards().unwrap() {
        println!("{:?}", rb);
    }
}
