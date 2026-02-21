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
}

fn main() -> arb::Result {
    let args = Args::parse();

    if !args.status && !args.reset && args.relays.is_empty() {
        Args::command().print_help()?;
        std::process::exit(2);
    }

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
        !args.disable_verification,
        args.port,
    )
}
