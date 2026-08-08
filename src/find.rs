//! Finding the relay boards attached to the USB bus.
//!
//! A caller names a board by the port it is plugged into; enumeration names one by
//! where it sits on the USB tree. [`Select`] is that choice, and [`Path`] is the
//! unambiguous half of it.

use std::collections::BTreeMap;
use std::fmt;

use rusb::UsbContext;

use crate::ch341a;
use crate::errors::{Error, Result};

/// Where a board sits on the USB tree: its bus, and the hub ports leading down to it.
///
/// The last hop is the board's port number, which is what [`Select::Port`] matches
/// on. That number is only unique among the ports of one hub, so two boards behind
/// two hubs can share it; the whole path never collides, which is what lets
/// enumeration hand back selectors that always resolve to the board they came from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Path {
    bus: u8,
    /// The hub ports leading down to the board, root hub first. libusb caps the
    /// depth at seven hops and answers `Overflow` rather than a longer path, so
    /// this is always the whole path.
    hops: Vec<u8>,
}

impl Path {
    /// Builds a path on `bus` from the hub ports leading down to the device.
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

    /// The board's port on the hub it is plugged into.
    ///
    /// Only a root hub has no port at all, and a root hub is never a relay board.
    fn port(&self) -> Option<u8> {
        self.hops.last().copied()
    }
}

/// The `lsusb -t` spelling: `1-1.3` is port 3 of the hub on port 1 of bus 1.
///
/// Deliberately the same notation the system tools use, because that is the only
/// way to tell several identical boards apart — they carry no serial number, so
/// `arb --list` beside `lsusb -t` is how an operator works out which is which.
impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.bus)?;

        for (hop, port) in self.hops.iter().enumerate() {
            f.write_str(if hop == 0 { "-" } else { "." })?;
            write!(f, "{port}")?;
        }

        Ok(())
    }
}

/// Which board a [`Board`](crate::Board) is a handle to.
///
/// Crate-private, which is what let it widen to whole paths without touching the
/// public API: callers name a board by port, enumeration names one by where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Select {
    /// Whichever board is attached, so long as there is only one.
    Any,
    /// The board on this port of whatever hub it hangs off — `usb.board(Some(port))`.
    Port(u8),
    /// One specific board, wherever it is. Only [`Usb::boards`](crate::Usb::boards)
    /// builds these.
    Path(Path),
}

impl Select {
    /// Whether the board at `path` is the one this names.
    fn matches(&self, path: &Path) -> bool {
        match self {
            Select::Any => true,
            Select::Port(port) => path.port() == Some(*port),
            Select::Path(selected) => path == selected,
        }
    }

    /// The port this names, if it names one.
    pub fn port(&self) -> Option<u8> {
        match self {
            Select::Any => None,
            Select::Port(port) => Some(*port),
            Select::Path(path) => path.port(),
        }
    }
}

impl fmt::Display for Select {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Select::Any => f.write_str("any board"),
            Select::Port(port) => write!(f, "port {port}"),
            Select::Path(path) => match path.port() {
                Some(port) => write!(f, "port {port} ({path})"),
                // Only a root hub has no port, and a root hub is never a board.
                None => write!(f, "{path}"),
            },
        }
    }
}

/// Every attached relay board, keyed by where it sits on the USB tree.
///
/// A map rather than a sorted list because `devices()` promises no order, and
/// enumeration that reshuffled between calls would make `boards()[0]` a different
/// board each time. Keying by path makes that ordering a property of the type
/// rather than a sort that can be deleted without a test noticing.
pub fn find_devices(context: &rusb::Context) -> Result<BTreeMap<Path, ch341a::Device>> {
    let mut found = BTreeMap::new();

    for device in context.devices()?.iter() {
        if ch341a::is_ch341a(&device)? {
            found.insert(Path::of(&device)?, device);
        }
    }

    Ok(found)
}

/// The one attached board `select` names.
pub fn find_device(context: &rusb::Context, select: &Select) -> Result<ch341a::Device> {
    let mut matching = find_devices(context)?
        .into_iter()
        .filter(|(path, _)| select.matches(path))
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
    fn paths_that_end_in_the_same_port_are_still_distinct() {
        // The whole reason enumeration names boards by path: `port_number` is the
        // port on the *parent hub*, so these two report the same one.
        let left = Path::new(1, [1, 3]);
        let right = Path::new(1, [2, 3]);

        assert_eq!(left.port(), right.port());
        assert_ne!(left, right);

        // Same trailing port on a different bus, and the same hops in a different
        // order, are both a different place on the tree.
        assert_ne!(Path::new(1, [3]), Path::new(2, [3]));
        assert_ne!(left, Path::new(1, [3, 1]));

        assert!(Select::Path(left.clone()).matches(&left));
        assert!(!Select::Path(left).matches(&right));
    }

    #[test]
    fn a_port_selector_matches_that_port_on_any_hub() {
        // Unchanged behaviour: `usb.board(Some(3))` still names port 3 wherever it
        // is, and a second board answering to it is what `MultipleFound` reports.
        assert!(Select::Port(3).matches(&Path::new(1, [3])));
        assert!(Select::Port(3).matches(&Path::new(2, [2, 3])));

        // The port is the last hop, not any hop along the way.
        assert!(!Select::Port(3).matches(&Path::new(1, [3, 1])));
        assert!(!Select::Port(3).matches(&Path::new(1, [4])));

        assert!(Select::Any.matches(&Path::new(4, [1, 2, 3])));
    }

    #[test]
    fn boards_are_ordered_by_bus_and_then_by_path() {
        // `find_devices` keys its map on this, which is what makes `boards()`
        // return the same board in the same position every time it is called.
        let mut paths = [
            Path::new(2, [1]),
            Path::new(1, [1, 3]),
            Path::new(1, [2]),
            Path::new(1, [1]),
        ];

        paths.sort_unstable();

        assert_eq!(
            paths,
            [
                Path::new(1, [1]),
                Path::new(1, [1, 3]),
                Path::new(1, [2]),
                Path::new(2, [1]),
            ]
        );
    }

    #[test]
    fn a_board_labels_itself_by_what_it_names() {
        assert_eq!(Select::Any.to_string(), "any board");
        assert_eq!(Select::Port(3).to_string(), "port 3");
        assert_eq!(
            Select::Path(Path::new(1, [1, 3])).to_string(),
            "port 3 (1-1.3)"
        );
        assert_eq!(Select::Path(Path::new(2, [4])).to_string(), "port 4 (2-4)");
    }

    #[test]
    fn a_path_renders_the_way_lsusb_spells_it() {
        // `arb --list` is read beside `lsusb -t`, so the notation has to match it
        // exactly: identical boards carry no serial, and this is all that tells
        // them apart.
        assert_eq!(Path::new(1, [3]).to_string(), "1-3");
        assert_eq!(Path::new(1, [1, 3]).to_string(), "1-1.3");
        assert_eq!(Path::new(2, [1, 2, 3]).to_string(), "2-1.2.3");
    }

    #[test]
    fn a_board_reports_the_port_it_names() {
        assert_eq!(Select::Any.port(), None);
        assert_eq!(Select::Port(3).port(), Some(3));
        assert_eq!(Select::Path(Path::new(1, [1, 3])).port(), Some(3));
    }
}
