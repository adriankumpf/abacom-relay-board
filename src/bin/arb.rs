use clap::{value_parser, Parser};

use std::io::{self, Write};

#[derive(Parser, Debug)]
#[command(name = "abacom-relay-board (arb)")]
struct Args {
    /// Gets relays status
    #[arg(short, long, requires = "relays")]
    status: bool,

    /// Resets the relay board
    #[arg(short, long, conflicts_with = "relays")]
    reset: bool,

    /// Disables the verifaction after activating relays
    #[arg(short, long, requires = "relays")]
    disable_verification: bool,

    /// Custom USB Port
    #[arg(short, long)]
    port: Option<u8>,

    /// The relays to activate
    #[arg(value_name = "RELAYS", default_value = "0", value_parser = value_parser!(u8).range(0..8))]
    relays: Vec<u8>,
}

fn main() -> arb::Result {
    let args = Args::parse();

    if args.status {
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

        writeln!(io::stdout(), "Active relays: {}", active_relays.join(" "))?;

        return Ok(());
    }

    if args.reset {
        return arb::reset(args.port);
    }

    arb::set_status(
        args.relays
            .iter()
            .filter(|&&r| r != 0)
            .fold(0, |acc, &r| acc | 1 << (r - 1)),
        args.disable_verification,
        args.port,
    )
}
