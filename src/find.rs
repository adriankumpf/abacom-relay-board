//! Finding the relay boards attached to the USB bus.
//!
//! A caller names a board by the port it is plugged into; enumeration names one by
//! where it sits on the USB tree. [`Select`] is that choice, and [`Location`] is the
//! unambiguous half of it.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use rusb::UsbContext;

use crate::ch341a;
use crate::errors::{Error, Result};

/// Where a board sits on the USB tree: its bus, and the hub ports leading down to it.
///
/// The last hop is the board's port number, which is what
/// [`Usb::board`](crate::Usb::board) matches on. That number is only unique among
/// the ports of one hub, so two boards behind two hubs can share it; the whole
/// location never collides, which is what lets enumeration hand back selectors that
/// always resolve to the board they came from.
///
/// # Naming a board across restarts
///
/// [`Display`](fmt::Display) and [`FromStr`] round-trip through the `1-1.3` spelling
/// that `lsusb -t` uses — bus, then the hub ports leading down to the board — so a
/// board found by [`Usb::boards`](crate::Usb::boards) can be written to a
/// configuration file and named again later with
/// [`Usb::board_at`](crate::Usb::board_at):
///
/// ```
/// use arb::Location;
///
/// let location: Location = "1-1.3".parse()?;
///
/// assert_eq!(location.bus(), 1);
/// assert_eq!(location.port(), Some(3));
/// assert_eq!(location.to_string(), "1-1.3");
/// # Ok::<(), arb::Error>(())
/// ```
///
/// It survives re-enumeration, which is why a board is named by this rather than by
/// `Device::address`: unplugging the board, or the `reset_device` a caller issues to
/// recover from a USB fault, reassigns the address but leaves the location alone.
/// Physically moving the board to another socket does change it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    bus: u8,
    /// The hub ports leading down to the board, root hub first. libusb caps the
    /// depth at seven hops and answers `Overflow` rather than a longer path, so
    /// this is always the whole path.
    hops: Vec<u8>,
}

impl Location {
    /// Builds a location on `bus` from the hub ports leading down to the device.
    fn new(bus: u8, hops: impl Into<Vec<u8>>) -> Self {
        Self {
            bus,
            hops: hops.into(),
        }
    }

    /// Where `device` sits on the USB tree.
    fn of(device: &ch341a::Device) -> Result<Self> {
        Ok(Self::new(device.bus_number(), device.port_numbers()?))
    }

    /// The USB bus the board is on.
    pub fn bus(&self) -> u8 {
        self.bus
    }

    /// The board's port on the hub it is plugged into.
    ///
    /// Only a root hub has no port at all, and a root hub is never a relay board.
    pub fn port(&self) -> Option<u8> {
        self.hops.last().copied()
    }
}

/// The `lsusb -t` spelling: `1-1.3` is port 3 of the hub on port 1 of bus 1.
impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.bus)?;

        for (hop, port) in self.hops.iter().enumerate() {
            f.write_str(if hop == 0 { "-" } else { "." })?;
            write!(f, "{port}")?;
        }

        Ok(())
    }
}

impl FromStr for Location {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let invalid = || Error::InvalidLocation(s.to_owned());

        let (bus, hops) = s.split_once('-').ok_or_else(invalid)?;

        // A hopless location is a root hub, which is never a relay board, so
        // `1` and `1-` are rejected rather than parsed into something that can
        // only ever fail to resolve.
        Ok(Self {
            bus: bus.parse().map_err(|_| invalid())?,
            hops: hops
                .split('.')
                .map(|hop| hop.parse().map_err(|_| invalid()))
                .collect::<Result<Vec<u8>>>()?,
        })
    }
}

/// Which board a [`Board`](crate::Board) is a handle to.
///
/// Crate-private, which is what let it widen to whole locations without touching the
/// public API: callers name a board by port, enumeration names one by where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Select {
    /// Whichever board is attached, so long as there is only one.
    Any,
    /// The board on this port of whatever hub it hangs off — `usb.board(Some(port))`.
    Port(u8),
    /// One specific board, wherever it is — [`Usb::boards`](crate::Usb::boards) and
    /// [`Usb::board_at`](crate::Usb::board_at) build these.
    At(Location),
}

impl Select {
    /// Whether the board at `location` is the one this names.
    fn matches(&self, location: &Location) -> bool {
        match self {
            Select::Any => true,
            Select::Port(port) => location.port() == Some(*port),
            Select::At(selected) => location == selected,
        }
    }

    /// The port this names, if it names one.
    pub fn port(&self) -> Option<u8> {
        match self {
            Select::Any => None,
            Select::Port(port) => Some(*port),
            Select::At(location) => location.port(),
        }
    }

    /// The location this names, if it names one unambiguously.
    pub fn location(&self) -> Option<&Location> {
        match self {
            Select::Any | Select::Port(_) => None,
            Select::At(location) => Some(location),
        }
    }
}

impl fmt::Display for Select {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Select::Any => f.write_str("any board"),
            Select::Port(port) => write!(f, "port {port}"),
            Select::At(location) => match location.port() {
                Some(port) => write!(f, "port {port} ({location})"),
                // Only a root hub has no port, and a root hub is never a board.
                None => write!(f, "{location}"),
            },
        }
    }
}

/// Every attached relay board, keyed by where it sits on the USB tree.
///
/// A map rather than a sorted list because `devices()` promises no order, and
/// enumeration that reshuffled between calls would make `boards()[0]` a different
/// board each time. Keying by location makes that ordering a property of the type
/// rather than a sort that can be deleted without a test noticing.
pub fn find_devices(context: &rusb::Context) -> Result<BTreeMap<Location, ch341a::Device>> {
    let mut found = BTreeMap::new();

    for device in context.devices()?.iter() {
        if ch341a::is_ch341a(&device)? {
            found.insert(Location::of(&device)?, device);
        }
    }

    Ok(found)
}

/// The one attached board `select` names.
pub fn find_device(context: &rusb::Context, select: &Select) -> Result<ch341a::Device> {
    let mut matching = find_devices(context)?
        .into_iter()
        .filter(|(location, _)| select.matches(location))
        .map(|(_, device)| device);

    let device = matching.next().ok_or(Error::NotFound)?;

    if matching.next().is_some() {
        return Err(Error::MultipleFound);
    }

    Ok(device)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_that_end_in_the_same_port_are_still_distinct() {
        // The whole reason enumeration names boards by location: `port_number` is
        // the port on the *parent hub*, so these two report the same one.
        let left = Location::new(1, [1, 3]);
        let right = Location::new(1, [2, 3]);

        assert_eq!(left.port(), right.port());
        assert_ne!(left, right);

        // Same trailing port on a different bus, and the same hops in a different
        // order, are both a different place on the tree.
        assert_ne!(Location::new(1, [3]), Location::new(2, [3]));
        assert_ne!(left, Location::new(1, [3, 1]));

        assert!(Select::At(left.clone()).matches(&left));
        assert!(!Select::At(left).matches(&right));
    }

    #[test]
    fn a_port_selector_matches_that_port_on_any_hub() {
        // Unchanged behaviour: `usb.board(Some(3))` still names port 3 wherever it
        // is, and a second board answering to it is what `MultipleFound` reports.
        assert!(Select::Port(3).matches(&Location::new(1, [3])));
        assert!(Select::Port(3).matches(&Location::new(2, [2, 3])));

        // The port is the last hop, not any hop along the way.
        assert!(!Select::Port(3).matches(&Location::new(1, [3, 1])));
        assert!(!Select::Port(3).matches(&Location::new(1, [4])));

        assert!(Select::Any.matches(&Location::new(4, [1, 2, 3])));
    }

    #[test]
    fn boards_are_ordered_by_bus_and_then_by_location() {
        // `find_devices` keys its map on this, which is what makes `boards()`
        // return the same board in the same position every time it is called.
        let mut locations = [
            Location::new(2, [1]),
            Location::new(1, [1, 3]),
            Location::new(1, [2]),
            Location::new(1, [1]),
        ];

        locations.sort_unstable();

        assert_eq!(
            locations,
            [
                Location::new(1, [1]),
                Location::new(1, [1, 3]),
                Location::new(1, [2]),
                Location::new(2, [1]),
            ]
        );
    }

    #[test]
    fn a_board_labels_itself_by_what_it_names() {
        assert_eq!(Select::Any.to_string(), "any board");
        assert_eq!(Select::Port(3).to_string(), "port 3");
        assert_eq!(
            Select::At(Location::new(1, [1, 3])).to_string(),
            "port 3 (1-1.3)"
        );
        assert_eq!(
            Select::At(Location::new(2, [4])).to_string(),
            "port 4 (2-4)"
        );
    }

    #[test]
    fn a_board_reports_the_port_it_names() {
        assert_eq!(Select::Any.port(), None);
        assert_eq!(Select::Port(3).port(), Some(3));
        assert_eq!(Select::At(Location::new(1, [1, 3])).port(), Some(3));
    }

    #[test]
    fn only_an_enumerated_board_reports_a_location() {
        // What makes a listed board storable and a port-selected one not: there is
        // no location to write down for `usb.board(Some(3))`, because the port it
        // names may resolve to a different board tomorrow.
        assert_eq!(Select::Any.location(), None);
        assert_eq!(Select::Port(3).location(), None);

        let location = Location::new(1, [1, 3]);
        assert_eq!(Select::At(location.clone()).location(), Some(&location));
    }

    #[test]
    fn a_location_round_trips_through_its_rendering() {
        // The point of the type: a board listed today must resolve to the same
        // board after a restart, which means `Display` and `FromStr` must agree.
        for location in [
            Location::new(1, [3]),
            Location::new(1, [1, 3]),
            Location::new(2, [1, 2, 3, 4, 5, 6, 7]),
            Location::new(255, [255]),
        ] {
            let rendered = location.to_string();

            assert_eq!(
                rendered.parse::<Location>().unwrap(),
                location,
                "{rendered}"
            );
        }
    }

    #[test]
    fn a_location_renders_the_way_lsusb_spells_it() {
        assert_eq!(Location::new(1, [3]).to_string(), "1-3");
        assert_eq!(Location::new(1, [1, 3]).to_string(), "1-1.3");
        assert_eq!(Location::new(2, [1, 2, 3]).to_string(), "2-1.2.3");
    }

    #[test]
    fn malformed_locations_are_rejected() {
        for input in [
            "", "1",      // a bus with no hops is a root hub, never a board
            "1-",     // trailing separator
            "1-1.",   // trailing hop separator
            "1-.1",   // leading hop separator
            "-1.3",   // no bus
            "1.3",    // hops without a bus
            "256-1",  // bus outside a byte
            "1-256",  // hop outside a byte
            "a-1.3",  // not a number
            "1-1.b",  // not a number
            "1-1..3", // empty hop
            "1-1.3 ", // stray whitespace
        ] {
            assert!(
                matches!(input.parse::<Location>(), Err(Error::InvalidLocation(got)) if got == input),
                "{input:?} should be rejected"
            );
        }
    }
}
