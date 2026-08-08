//! Relay addressing.
//!
//! The board exposes eight relays, numbered 1 to 8 on the case. The A6275 shift
//! register addresses them as an 8-bit mask, with relay 1 in the least significant
//! bit. `Relay::bit` is the single place that mapping is written down.

use std::array;
use std::fmt;
use std::iter::FusedIterator;
use std::ops::{BitOr, BitOrAssign};

use crate::errors::{Error, Result};

/// A single relay, numbered 1 to 8 as labelled on the board.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relay {
    One = 1,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
}

impl Relay {
    /// Every relay, in ascending order.
    pub const ALL: [Relay; 8] = [
        Relay::One,
        Relay::Two,
        Relay::Three,
        Relay::Four,
        Relay::Five,
        Relay::Six,
        Relay::Seven,
        Relay::Eight,
    ];

    /// Returns the relay's number, as labelled on the board.
    ///
    /// ```
    /// assert_eq!(arb::Relay::Three.number(), 3);
    /// ```
    pub const fn number(self) -> u8 {
        self as u8
    }

    /// Returns the relay's bit in the shift register mask.
    const fn bit(self) -> u8 {
        1 << (self as u8 - 1)
    }
}

impl TryFrom<u8> for Relay {
    type Error = Error;

    /// Converts a relay number into a [`Relay`].
    ///
    /// ```
    /// assert_eq!(arb::Relay::try_from(3).unwrap(), arb::Relay::Three);
    /// assert!(arb::Relay::try_from(9).is_err());
    /// ```
    fn try_from(number: u8) -> Result<Self> {
        Relay::ALL
            .into_iter()
            .find(|relay| relay.number() == number)
            .ok_or(Error::InvalidRelay(number))
    }
}

impl fmt::Display for Relay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.number())
    }
}

/// A set of relays, stored as the board's 8-bit mask.
///
/// Every one of the 256 masks is a valid set, so this is a total representation
/// rather than a validated one — the type exists to name the mapping and carry it,
/// not to reject values.
///
/// ```
/// use arb::{Relay, Relays};
///
/// let relays = Relay::One | Relay::Three;
///
/// assert!(relays.contains(Relay::One));
/// assert!(!relays.contains(Relay::Two));
/// assert_eq!(relays.bits(), 0b0000_0101);
/// ```
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Relays(u8);

impl Relays {
    /// No relays active — every relay off.
    pub const NONE: Self = Self(0);

    /// Every relay active.
    pub const ALL: Self = Self(u8::MAX);

    /// Wraps a raw shift register mask, where bit 0 is relay 1.
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// Returns the raw shift register mask.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns whether no relay is active.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns whether `relay` is in the set.
    pub const fn contains(self, relay: Relay) -> bool {
        self.0 & relay.bit() != 0
    }

    /// Adds `relay` to the set.
    pub const fn insert(&mut self, relay: Relay) {
        self.0 |= relay.bit();
    }

    /// Removes `relay` from the set.
    pub const fn remove(&mut self, relay: Relay) {
        self.0 &= !relay.bit();
    }

    /// Returns the relays in the set, in ascending order.
    ///
    /// ```
    /// use arb::{Relay, Relays};
    ///
    /// let relays = Relays::from_bits(0b0011_1011);
    ///
    /// assert_eq!(relays.iter().map(Relay::number).collect::<Vec<_>>(), [1, 2, 4, 5, 6]);
    /// ```
    pub fn iter(self) -> RelayIter {
        RelayIter {
            relays: self,
            remaining: Relay::ALL.into_iter(),
        }
    }
}

/// An iterator over the relays in a [`Relays`], in ascending order.
pub struct RelayIter {
    relays: Relays,
    remaining: array::IntoIter<Relay, 8>,
}

impl Iterator for RelayIter {
    type Item = Relay;

    fn next(&mut self) -> Option<Relay> {
        let relays = self.relays;

        self.remaining.find(|&relay| relays.contains(relay))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();

        (len, Some(len))
    }
}

impl ExactSizeIterator for RelayIter {
    fn len(&self) -> usize {
        self.remaining
            .as_slice()
            .iter()
            .filter(|&&relay| self.relays.contains(relay))
            .count()
    }
}

impl FusedIterator for RelayIter {}

impl From<Relay> for Relays {
    fn from(relay: Relay) -> Self {
        Self(relay.bit())
    }
}

impl FromIterator<Relay> for Relays {
    fn from_iter<I: IntoIterator<Item = Relay>>(relays: I) -> Self {
        relays
            .into_iter()
            .fold(Self::NONE, |set, relay| set | relay)
    }
}

impl IntoIterator for Relays {
    type Item = Relay;
    type IntoIter = RelayIter;

    fn into_iter(self) -> RelayIter {
        self.iter()
    }
}

impl BitOr for Relay {
    type Output = Relays;

    fn bitor(self, rhs: Relay) -> Relays {
        Relays(self.bit() | rhs.bit())
    }
}

impl BitOr<Relay> for Relays {
    type Output = Relays;

    fn bitor(self, rhs: Relay) -> Relays {
        Relays(self.0 | rhs.bit())
    }
}

impl BitOr for Relays {
    type Output = Relays;

    fn bitor(self, rhs: Relays) -> Relays {
        Relays(self.0 | rhs.0)
    }
}

impl BitOrAssign<Relay> for Relays {
    fn bitor_assign(&mut self, rhs: Relay) {
        self.insert(rhs);
    }
}

impl BitOrAssign for Relays {
    fn bitor_assign(&mut self, rhs: Relays) {
        self.0 |= rhs.0;
    }
}

/// Renders the active relay numbers, separated by spaces, or `none` if no relay
/// is active.
///
/// The empty set is spelled out rather than rendered as the empty string, so that
/// it reads as a value wherever it is interpolated.
impl fmt::Display for Relays {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("none");
        }

        for (i, relay) in self.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }

            write!(f, "{relay}")?;
        }

        Ok(())
    }
}

impl fmt::Debug for Relays {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(relays: Relays) -> Vec<u8> {
        relays.iter().map(Relay::number).collect()
    }

    #[test]
    fn relay_n_is_bit_n_minus_one() {
        for relay in Relay::ALL {
            assert_eq!(relay.bit(), 1 << (relay.number() - 1));
            assert_eq!(numbers(relay.into()), vec![relay.number()]);
        }
    }

    #[test]
    fn relay_numbers_round_trip() {
        for relay in Relay::ALL {
            assert_eq!(Relay::try_from(relay.number()).unwrap(), relay);
        }
    }

    #[test]
    fn relay_numbers_outside_one_to_eight_are_rejected() {
        for number in [0, 9, 10, u8::MAX] {
            assert!(matches!(
                Relay::try_from(number),
                Err(Error::InvalidRelay(n)) if n == number
            ));
        }
    }

    #[test]
    fn relays_combine_into_one_mask() {
        let relays = Relay::One | Relay::Two | Relay::Four | Relay::Five | Relay::Six;

        assert_eq!(relays.bits(), 0b0011_1011);
        assert_eq!(numbers(relays), vec![1, 2, 4, 5, 6]);
    }

    #[test]
    fn repeating_a_relay_sets_its_bit_once() {
        assert_eq!((Relay::Three | Relay::Three).bits(), 0b0000_0100);
    }

    #[test]
    fn sets_can_be_unioned_and_extended_in_place() {
        let mut relays = Relay::One | Relay::Three;

        relays |= Relay::Five;
        relays |= Relay::Two | Relay::Four;

        assert_eq!(relays, Relays::from_bits(0b0001_1111));
        assert_eq!(relays | Relays::ALL, Relays::ALL);
    }

    #[test]
    fn iterating_reports_its_exact_remaining_length() {
        let mut iter = Relays::from_bits(0b0011_1011).iter();

        for expected in (0..5).rev() {
            iter.next();

            assert_eq!(iter.len(), expected);
            assert_eq!(iter.size_hint(), (expected, Some(expected)));
        }
    }

    #[test]
    fn every_mask_round_trips_through_its_relays() {
        for bits in 0..=u8::MAX {
            let relays = Relays::from_bits(bits);

            assert_eq!(relays.iter().collect::<Relays>(), relays);
            assert_eq!(relays.bits(), bits);
        }
    }

    #[test]
    fn none_and_all_are_the_mask_extremes() {
        assert_eq!(Relays::NONE.bits(), 0);
        assert_eq!(Relays::ALL.bits(), u8::MAX);
        assert!(Relays::NONE.is_empty());
        assert!(!Relays::ALL.is_empty());
        assert_eq!(numbers(Relays::ALL), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(numbers(Relays::NONE), Vec::<u8>::new());
    }

    #[test]
    fn collecting_no_relays_yields_the_empty_set() {
        assert_eq!(std::iter::empty().collect::<Relays>(), Relays::NONE);
    }

    #[test]
    fn insert_and_remove_toggle_membership() {
        let mut relays = Relays::NONE;

        relays.insert(Relay::Five);
        assert!(relays.contains(Relay::Five));

        relays.remove(Relay::Five);
        assert!(!relays.contains(Relay::Five));
        assert_eq!(relays, Relays::NONE);
    }

    #[test]
    fn removing_a_relay_leaves_the_others() {
        let mut relays = Relays::ALL;

        relays.remove(Relay::One);

        assert_eq!(relays.bits(), 0b1111_1110);
    }

    #[test]
    fn display_lists_the_active_relay_numbers() {
        assert_eq!(Relays::from_bits(0b0011_1011).to_string(), "1 2 4 5 6");
        assert_eq!(Relays::NONE.to_string(), "none");
        assert_eq!(Relays::ALL.to_string(), "1 2 3 4 5 6 7 8");
    }

    #[test]
    fn debug_shows_the_set_contents() {
        assert_eq!(format!("{:?}", Relay::One | Relay::Three), "{One, Three}");
    }
}
