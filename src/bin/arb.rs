use clap::{CommandFactory, Parser, value_parser};

use std::io::{self, Write};

#[derive(Parser, Debug)]
#[command(name = "abacom-relay-board (arb)")]
struct Args {
    /// Gets relays status
    #[arg(short, long, conflicts_with_all = ["relays", "reset", "disable_verification"])]
    status: bool,

    /// Resets the relay board
    #[arg(short, long, conflicts_with_all = ["relays", "disable_verification"])]
    reset: bool,

    /// Disables the verification after activating relays
    #[arg(short, long)]
    disable_verification: bool,

    /// Custom USB Port
    #[arg(short, long)]
    port: Option<u8>,

    /// The relays to activate
    #[arg(value_name = "RELAYS", value_parser = value_parser!(u8).range(0..=8))]
    relays: Vec<u8>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("arb: {e}");
        std::process::exit(1);
    }
}

/// Converts relay numbers into the bitmask the library expects.
///
/// Relay `n` is bit `n - 1`. `0` is not a relay and contributes no bits, so
/// `arb 0` yields an empty mask — which is how "turn everything off" is spelled.
fn relays_to_mask(relays: &[u8]) -> u8 {
    relays
        .iter()
        .copied()
        .filter(|&relay| relay != 0)
        .fold(0, |mask, relay| mask | 1 << (relay - 1))
}

/// Returns the numbers of the relays set in `status`, in ascending order.
fn active_relays(status: u8) -> impl Iterator<Item = u8> {
    (1..=8u8).filter(move |relay| status & (1 << (relay - 1)) != 0)
}

fn run() -> arb::Result {
    let args = Args::parse();

    if !args.status && !args.reset && args.relays.is_empty() {
        Args::command().print_help()?;
        std::process::exit(2);
    }

    if args.status {
        let status = arb::get_status(args.port)?;

        let active: Vec<_> = active_relays(status).map(|r| r.to_string()).collect();

        writeln!(io::stdout(), "Active relays: {}", active.join(" "))?;

        return Ok(());
    }

    if args.reset {
        return arb::reset(args.port);
    }

    arb::set_status(
        relays_to_mask(&args.relays),
        !args.disable_verification,
        args.port,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("arb").chain(args.iter().copied()))
    }

    #[test]
    fn bare_invocation_parses_with_empty_relays() {
        let args = parse(&[]).unwrap();
        assert!(!args.status);
        assert!(!args.reset);
        assert!(args.relays.is_empty());
    }

    #[test]
    fn status_flag() {
        let args = parse(&["--status"]).unwrap();
        assert!(args.status);
    }

    #[test]
    fn reset_flag() {
        let args = parse(&["--reset"]).unwrap();
        assert!(args.reset);
    }

    #[test]
    fn relay_args() {
        let args = parse(&["1", "3", "5"]).unwrap();
        assert_eq!(args.relays, vec![1, 3, 5]);
    }

    #[test]
    fn relay_zero_deactivates_all() {
        let args = parse(&["0"]).unwrap();
        assert_eq!(args.relays, vec![0]);
    }

    #[test]
    fn disable_verification_with_relays() {
        let args = parse(&["-d", "1", "2"]).unwrap();
        assert!(args.disable_verification);
        assert_eq!(args.relays, vec![1, 2]);
    }

    #[test]
    fn port_option() {
        let args = parse(&["--port", "3", "1"]).unwrap();
        assert_eq!(args.port, Some(3));
    }

    #[test]
    fn relay_out_of_range() {
        assert!(parse(&["9"]).is_err());
    }

    #[test]
    fn status_conflicts_with_relays() {
        assert!(parse(&["--status", "1", "2"]).is_err());
    }

    #[test]
    fn status_conflicts_with_reset() {
        assert!(parse(&["--status", "--reset"]).is_err());
    }

    #[test]
    fn status_conflicts_with_disable_verification() {
        assert!(parse(&["--status", "-d"]).is_err());
    }

    #[test]
    fn reset_conflicts_with_relays() {
        assert!(parse(&["--reset", "1", "2"]).is_err());
    }

    #[test]
    fn reset_conflicts_with_disable_verification() {
        assert!(parse(&["--reset", "-d"]).is_err());
    }

    fn active(status: u8) -> Vec<u8> {
        active_relays(status).collect()
    }

    #[test]
    fn relay_n_is_bit_n_minus_one() {
        // Relay 1 is the least significant bit, relay 8 the most significant.
        for relay in 1..=8u8 {
            assert_eq!(relays_to_mask(&[relay]), 1 << (relay - 1));
            assert_eq!(active(1 << (relay - 1)), vec![relay]);
        }
    }

    #[test]
    fn relays_combine_into_one_mask() {
        assert_eq!(relays_to_mask(&[1, 2, 4, 5, 6]), 0b0011_1011);
        assert_eq!(active(0b0011_1011), vec![1, 2, 4, 5, 6]);
    }

    #[test]
    fn all_relays_fill_the_mask() {
        assert_eq!(relays_to_mask(&[1, 2, 3, 4, 5, 6, 7, 8]), u8::MAX);
        assert_eq!(active(u8::MAX), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn an_empty_mask_turns_everything_off() {
        assert_eq!(relays_to_mask(&[]), 0);
        assert_eq!(relays_to_mask(&[0]), 0);
        assert_eq!(active(0), Vec::<u8>::new());
    }

    #[test]
    fn relay_zero_is_ignored_beside_other_relays() {
        // `0` only means "all off" on its own; listed alongside real relays it
        // drops out and the others still activate.
        assert_eq!(relays_to_mask(&[0, 3]), relays_to_mask(&[3]));
    }

    #[test]
    fn repeating_a_relay_sets_its_bit_once() {
        assert_eq!(relays_to_mask(&[3, 3, 3]), 0b0000_0100);
    }
}
