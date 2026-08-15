use clap::{ArgGroup, CommandFactory, Parser, value_parser};

use std::error::Error;
use std::io::{self, Write};

use arb::{Relay, Relays, Verify};

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

    /// Custom USB Port
    #[arg(short, long)]
    port: Option<u8>,

    /// The relays to activate
    #[arg(value_name = "RELAYS", value_parser = value_parser!(u8).range(0..=8))]
    relays: Vec<u8>,
}

/// What an invocation asks for, which is exactly one thing.
///
/// The only place the flags are read as modes, so the dispatch and the "no mode
/// given, print the help" branch cannot drift apart.
#[derive(Debug, PartialEq)]
enum Mode {
    Status,
    List,
    Reset,
    Relays,
}

impl Args {
    /// The mode given, if any. The `mode` group makes them mutually exclusive, so a
    /// parsed `Args` names at most one.
    fn mode(&self) -> Option<Mode> {
        if self.status {
            Some(Mode::Status)
        } else if self.list {
            Some(Mode::List)
        } else if self.reset {
            Some(Mode::Reset)
        } else if !self.relays.is_empty() {
            Some(Mode::Relays)
        } else {
            None
        }
    }
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

    let Some(mode) = args.mode() else {
        Args::command().print_help()?;
        std::process::exit(2);
    };

    // After the help branch: initialising libusb here would make a bare `arb`
    // fail with a USB error instead of printing its help.
    let usb = arb::Usb::new()?;

    let board = usb.board(args.port);

    match mode {
        Mode::List => {
            // No board prints nothing rather than erroring, so the output stays
            // something a script can read line by line.
            for found in usb.boards()? {
                writeln!(io::stdout(), "{found}")?;
            }
        }

        Mode::Status => {
            // The library keeps the check off the read path for callers that read
            // thousands of times; a one-shot CLI is the opposite case. It pays
            // another ~1.1 ms on top of the ~6.5 ms this process already spent
            // initialising libusb, and printing state that a flaky board invented is
            // exactly the failure a person reading it wants caught.
            //
            // One claim: the relays printed are the ones the check vouched for.
            let relays = board.self_test()?;

            writeln!(io::stdout(), "Active relays: {relays}")?;
        }

        Mode::Reset => board.reset_device()?,

        Mode::Relays => {
            let verify = if args.disable_verification {
                Verify::Disabled
            } else {
                Verify::Enabled
            };

            board.set_relays(requested_relays(&args.relays)?, verify)?;
        }
    }

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

    #[test]
    fn every_flag_names_the_mode_it_runs() {
        assert_eq!(parse(&["--status"]).unwrap().mode(), Some(Mode::Status));
        assert_eq!(parse(&["--list"]).unwrap().mode(), Some(Mode::List));
        assert_eq!(parse(&["--reset"]).unwrap().mode(), Some(Mode::Reset));
        assert_eq!(parse(&["1", "2"]).unwrap().mode(), Some(Mode::Relays));
        assert_eq!(parse(&["0"]).unwrap().mode(), Some(Mode::Relays));
    }

    #[test]
    fn an_invocation_without_a_mode_names_none() {
        // What makes a bare `arb` print its help, and `--port` alone with it: a
        // modifier is not a mode.
        assert_eq!(parse(&[]).unwrap().mode(), None);
        assert_eq!(parse(&["--port", "3"]).unwrap().mode(), None);
    }

    #[test]
    fn the_mode_group_holds_exactly_the_flags_mode_reads() {
        // `mode()` and the `ArgGroup` name the modes independently, and nothing makes
        // them agree: a mode added to one and not the other still compiles. Forgetting
        // the group is the silent half — two modes would then parse together and the
        // first-match-wins chain would drop one without a word.
        let command = Args::command();
        let group = command
            .get_groups()
            .find(|group| group.get_id() == "mode")
            .expect("the mode group");

        let mut ids: Vec<_> = group.get_args().map(|id| id.as_str()).collect();
        ids.sort_unstable();

        assert_eq!(ids, ["list", "relays", "reset", "status"]);
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
