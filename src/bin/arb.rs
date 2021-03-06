use structopt::StructOpt;

use std::io::{self, Write};

#[derive(StructOpt, Debug)]
#[structopt(name = "abacom-relay-board (arb)")]
struct Args {
    /// Gets relays status
    #[structopt(short, long, requires = "RELAYS")]
    status: bool,

    /// Resets the relay board
    #[structopt(short, long, conflicts_with = "RELAYS")]
    reset: bool,

    /// Disables the verifaction after activating relays
    #[structopt(short, long, requires = "RELAYS")]
    disable_verification: bool,

    /// Custom USB Port
    #[structopt(short, long)]
    port: Option<u8>,

    /// The relays to activate
    #[structopt(name = "RELAYS", default_value = "0", possible_values = &["0", "1", "2", "3", "4", "5", "6", "7", "8"])]
    relays: Vec<u8>,
}

fn main() -> arb::Result {
    let args = Args::from_args();

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
