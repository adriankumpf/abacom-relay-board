use clap::{ArgGroup, CommandFactory, Parser, value_parser};

use std::error::Error;
use std::io::{self, Write};
use std::num::IntErrorKind;
use std::str::FromStr;

use arb::{Location, Relay, Relays, Usb, Verify};

/// Which board `--port` names.
///
/// A bare number is a port, exactly as before. Anything else is parsed as a
/// [`Location`], so the `port 3 (1-1.3)` that `--list` prints can be fed straight
/// back in — otherwise `--list` could name a board the CLI had no way to address.
/// The two never collide: a location always contains a `-`, so it can never parse
/// as a port number.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Port(u8),
    At(Location),
}

impl FromStr for Target {
    // A `String` rather than `arb::Error`, so that a number too large to be a port
    // can be answered as the port it was meant to be rather than as the location it
    // is not: `arb --port 256` wants "port numbers run from 0 to 255", not advice
    // about writing `1-1.3`.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.parse::<u8>() {
            Ok(port) => Ok(Target::Port(port)),
            // The integer parser has already said whether this was a number at
            // all, so asking it rather than re-scanning the digits leaves one
            // notion of "is a port" instead of two to keep in step.
            Err(e) if *e.kind() == IntErrorKind::PosOverflow => {
                Err(format!("port numbers run from 0 to 255, got `{s}`"))
            }
            Err(_) => s
                .parse()
                .map(Target::At)
                .map_err(|e: arb::Error| e.to_string()),
        }
    }
}

// The modes are mutually exclusive, which a group states once rather than pairwise
// on each of them. `disable_verification` and `port` are modifiers, not modes, so
// they name the modes they do not apply to.
#[derive(Parser, Debug)]
#[command(name = "abacom-relay-board (arb)")]
#[command(group(ArgGroup::new("mode").args(["status", "list", "reset", "relays"])))]
struct Args {
    /// Gets relays status
    #[arg(short, long)]
    status: bool,

    /// Lists the attached relay boards
    #[arg(short, long, conflicts_with = "port")]
    list: bool,

    /// Performs a USB reset on the relay board
    #[arg(short, long)]
    reset: bool,

    /// Disables the verification after activating relays
    #[arg(short, long, conflicts_with_all = ["status", "list", "reset"])]
    disable_verification: bool,

    /// Which board: a port number, or a location like `1-1.3` from --list
    #[arg(short, long, value_name = "PORT|LOCATION")]
    port: Option<Target>,

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

/// Collects the relay numbers given on the command line.
///
/// `0` is the CLI's way of spelling "turn everything off": it is not a relay, so
/// it contributes nothing and `arb 0` yields [`Relays::NONE`]. Clap has already
/// rejected anything outside `0..=8`, so the conversion cannot fail in practice.
fn requested_relays(numbers: &[u8]) -> arb::Result<Relays> {
    numbers
        .iter()
        .copied()
        .filter(|&number| number != 0)
        .map(Relay::try_from)
        .collect()
}

/// The CLI's errors: `arb::Error` from the library, `io::Error` from writing to
/// stdout. Both are only ever displayed, so a boxed trait object is enough and
/// the library needs no I/O variant of its own.
fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if !args.status && !args.list && !args.reset && args.relays.is_empty() {
        Args::command().print_help()?;
        std::process::exit(2);
    }

    // After the help branch: initialising libusb here would make a bare `arb`
    // fail with a USB error instead of printing its help.
    let usb = Usb::new()?;

    if args.list {
        // No board prints nothing rather than erroring, so the output stays
        // something a script can read line by line.
        for board in usb.boards()? {
            writeln!(io::stdout(), "{board}")?;
        }

        return Ok(());
    }

    let board = match args.port {
        Some(Target::Port(port)) => usb.board(Some(port)),
        Some(Target::At(location)) => usb.board_at(location),
        None => usb.board(None),
    };

    if args.status {
        // The library keeps the check off the read path for callers that read
        // thousands of times; a one-shot CLI is the opposite case. It pays another
        // ~2.3 ms on top of the ~6.5 ms this process already spent initialising
        // libusb, and printing state that a flaky board invented is exactly the
        // failure a person reading it wants caught.
        board.self_test()?;

        let relays = board.relays()?;

        writeln!(io::stdout(), "Active relays: {relays}")?;

        return Ok(());
    }

    if args.reset {
        board.reset_device()?;

        return Ok(());
    }

    let verify = if args.disable_verification {
        Verify::Disabled
    } else {
        Verify::Enabled
    };

    board.set_relays(requested_relays(&args.relays)?, verify)?;

    Ok(())
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
    fn list_flag() {
        let args = parse(&["--list"]).unwrap();
        assert!(args.list);
    }

    #[test]
    fn list_conflicts_with_every_other_mode() {
        // Including `--port`: listing is how you find out which port to give.
        assert!(parse(&["--list", "--status"]).is_err());
        assert!(parse(&["--list", "--reset"]).is_err());
        assert!(parse(&["--list", "--port", "3"]).is_err());
        assert!(parse(&["--list", "1", "2"]).is_err());
        assert!(parse(&["--list", "-d"]).is_err());
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
        assert_eq!(args.port, Some(Target::Port(3)));
    }

    #[test]
    fn port_option_accepts_a_location() {
        // `--list` prints `port 3 (1-1.3)`; the parenthesised half has to be
        // something `--port` takes, or listing names a board you cannot address.
        let args = parse(&["--port", "1-1.3", "--status"]).unwrap();

        assert_eq!(args.port, Some(Target::At("1-1.3".parse().unwrap())));
    }

    #[test]
    fn a_bare_number_is_still_a_port() {
        // The disambiguation rule, pinned: no location parses as a port and no
        // port as a location, because a location always carries a `-`.
        assert_eq!("3".parse::<Target>().unwrap(), Target::Port(3));
        assert_eq!("255".parse::<Target>().unwrap(), Target::Port(255));

        assert!(matches!("1-3".parse::<Target>().unwrap(), Target::At(_)));
    }

    #[test]
    fn port_option_rejects_nonsense() {
        assert!(parse(&["--port", "256", "--status"]).is_err());
        assert!(parse(&["--port", "1-", "--status"]).is_err());
        assert!(parse(&["--port", "eth0", "--status"]).is_err());
        assert!(parse(&["--port", "", "--status"]).is_err());
    }

    #[test]
    fn an_out_of_range_port_is_answered_as_a_port() {
        // 256 is neither a port nor a location. Falling through to the location
        // parser would answer it with advice about writing `1-1.3`, which is not
        // what someone who typed a number was reaching for.
        let error = "256".parse::<Target>().unwrap_err();

        assert!(error.contains("port numbers run from 0 to 255"), "{error}");

        // A location is still answered as a location.
        let error = "1-".parse::<Target>().unwrap_err();

        assert!(error.contains("invalid board location"), "{error}");
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

    fn requested(numbers: &[u8]) -> Relays {
        requested_relays(numbers).unwrap()
    }

    #[test]
    fn relay_numbers_become_the_matching_relays() {
        assert_eq!(
            requested(&[1, 2, 4, 5, 6]),
            Relay::One | Relay::Two | Relay::Four | Relay::Five | Relay::Six
        );
    }

    #[test]
    fn zero_or_no_relays_turns_everything_off() {
        assert_eq!(requested(&[0]), Relays::NONE);
        assert_eq!(requested(&[]), Relays::NONE);
    }

    #[test]
    fn relay_zero_is_ignored_beside_other_relays() {
        // `0` only means "all off" on its own; listed alongside real relays it
        // drops out and the others still activate.
        assert_eq!(requested(&[0, 3]), requested(&[3]));
    }

    #[test]
    fn repeating_a_relay_activates_it_once() {
        assert_eq!(requested(&[3, 3, 3]), Relay::Three.into());
    }

    #[test]
    fn out_of_range_relay_numbers_are_rejected() {
        // Unreachable through clap, which caps the value at 8, but the conversion
        // must not silently drop or misplace the relay if that guard ever moves.
        assert!(matches!(
            requested_relays(&[9]),
            Err(arb::Error::InvalidRelay(9))
        ));
    }
}
