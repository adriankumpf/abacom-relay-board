//! Library for controlling the ABACOM CH341A USB relay board.
//!
//! The board uses a CH341A USB-to-parallel chip to drive an Allegro A6275 shift register,
//! which in turn controls 8 relays. Communication is SPI-like: bits are clocked into the
//! A6275 serially (DATA + CLK), then latched to the outputs.
//!
//! # Relay addressing
//!
//! Relays are named by [`Relay`] and addressed as a set by [`Relays`]. The board
//! stores that set as an 8-bit shift register mask, with relay 1 in the least
//! significant bit, but only [`Relays::from_bits`] and [`Relays::bits`] expose it.
//!
//! # Telling boards apart
//!
//! These boards carry no identity of their own. The CH341A reports no serial
//! number and no manufacturer or product strings at all — every one answers to the
//! same `1a86:5512` and nothing else — so the only thing that distinguishes two of
//! them is *where each is plugged in*.
//!
//! That has a consequence worth stating plainly: **if identical boards are
//! unplugged and returned to different sockets, nothing in software can tell that
//! they were swapped.** A configuration naming port 3 will drive whatever is now on
//! port 3, and it will do so silently. Where that matters — and it does, if the
//! relays drive things that must not be actuated by mistake — label the cables.
//! Software cannot cover this one.
//!
//! Within a running system the addressing is stable: a port number is physical, so
//! it survives the re-enumeration a [`Board::reset_device`] causes, where a USB
//! device address would not. [`Usb::boards`] enumerates in a stable order, and
//! `arb --list` prints each board as `port 3 (1-1.3)`, carrying the same `1-1.3`
//! notation `lsusb -t` uses, so the two can be read side by side.
//!
//! # Examples
//!
//! ```no_run
//! use arb::{Relay, Relays, Usb, Verify};
//!
//! // Initialise libusb once and keep it: this is the expensive part
//! let usb = Usb::new().unwrap();
//! let board = usb.board(None);
//!
//! // Confirm the board is answering, worth doing once and not on every read. The
//! // check has to read the register before it can test it, so it hands back what
//! // it found there and a separate `relays()` call would only ask again.
//! let active = board.self_test().unwrap();
//! println!("active at startup: {active}");
//!
//! // Activate relays 1 and 3
//! board.set_relays(Relay::One | Relay::Three, Verify::Enabled).unwrap();
//!
//! // Read back the current state
//! for relay in board.relays().unwrap() {
//!     println!("relay {relay} is active");
//! }
//!
//! // Turn everything off
//! board.set_relays(Relays::NONE, Verify::Enabled).unwrap();
//! ```

use std::fmt;

mod a6275;
mod ch341a;
mod errors;
mod find;
mod relays;

use self::a6275::A6275;
use self::ch341a::Ch341a;
use self::find::{Select, find_device, find_devices};

pub use self::errors::{Error, Result};
pub use self::relays::{Relay, RelayIter, Relays};

/// Whether [`Board::set_relays`] reads the shift register back to confirm the write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verify {
    /// Read the shift register back and fail on a mismatch.
    #[default]
    Enabled,
    /// Latch the relays without confirming.
    Disabled,
}

/// A libusb context: how relay boards are found.
///
/// Initialising it is by far the most expensive part of talking to a board, so
/// create one and keep it. It is cheap to clone (reference-counted) and safe to
/// share across threads. It claims nothing and opens nothing, so contexts never
/// conflict with each other or with another application using the same board.
///
/// # Recovering from a soured context
///
/// A held context is not self-healing: if the USB controller resets or the host
/// suspends, a `Usb` may go permanently sour where the per-call context of 0.7.1
/// would have been rebuilt anyway.
///
/// Do not try to detect that. No error reliably distinguishes a dead context from
/// a transient fault — libusb does not report one, so any classification here
/// would be guesswork. `Usb::new` costs ~6.5 ms and failures are rare, so the
/// robust policy is the simple one: **drop the context and build a new one after
/// any failed operation**, with no threshold to tune and nothing to classify. The
/// happy path is untouched.
///
/// Worth logging when that happens. This failure mode is inherited belief rather
/// than something observed against this hardware — no consumer held a long-lived
/// context before 0.8.0 — so if a rebuild never once restores service, this
/// warning can come out.
///
/// # Example
///
/// ```no_run
/// let usb = arb::Usb::new().unwrap();
///
/// let relays = usb.board(None).relays().unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct Usb(rusb::Context);

impl Usb {
    /// Initialises libusb.
    ///
    /// # Errors
    ///
    /// * [`Error::Usb`] — libusb could not be initialised
    pub fn new() -> Result<Self> {
        Ok(Self(rusb::Context::new()?))
    }

    /// Returns the board on `port`, or the only attached board if `None`.
    ///
    /// Resolves nothing: the board is looked up when a method is called on it, so
    /// this cannot fail and a [`Board`] may outlive — or predate — the device it
    /// names.
    ///
    /// A port number is the board's port on the hub it is plugged into, so it is
    /// unique only among that hub's ports: two boards behind two hubs can both be
    /// on port 3, and this then resolves to [`Error::MultipleFound`].
    /// [`Usb::boards`] is the way out, and names each board unambiguously.
    ///
    /// # Arguments
    ///
    /// * `port` — USB port number to select a specific board when multiple are connected.
    pub fn board(&self, port: Option<u8>) -> Board {
        let select = match port {
            Some(port) => Select::Port(port),
            None => Select::Any,
        };

        Board {
            usb: self.clone(),
            select,
        }
    }

    /// Returns every attached relay board, in a stable order.
    ///
    /// Each [`Board`] names one specific device by where it sits on the USB tree
    /// rather than by port, so enumeration never hands back the ambiguity described
    /// on [`Usb::board`]. They are otherwise the lazy boards it returns: nothing is
    /// opened, nothing is claimed, and each is resolved afresh per call — so one may
    /// stop resolving if its board is unplugged.
    ///
    /// An empty vector means no board is attached. Enumeration answering "none" is
    /// not a failure, so this does not return [`Error::NotFound`].
    ///
    /// # Errors
    ///
    /// * [`Error::Usb`] — the USB device list could not be read
    ///
    /// # Example
    ///
    /// ```no_run
    /// let usb = arb::Usb::new().unwrap();
    ///
    /// for board in usb.boards().unwrap() {
    ///     println!("{board}: {}", board.relays().unwrap());
    /// }
    /// ```
    pub fn boards(&self) -> Result<Vec<Board>> {
        Ok(find_devices(&self.0)?
            .into_keys()
            .map(|path| Board {
                usb: self.clone(),
                select: Select::Path(path),
            })
            .collect())
    }
}

/// One relay board, found and claimed afresh for the duration of every call.
///
/// Holds no device and no claim between calls, so several `Board`s — in this
/// process or in another application — can drive the same hardware. Two calls
/// that do overlap are not serialised: the loser gets [`Error::Busy`] and should
/// retry.
///
/// # Atomicity
///
/// One call is atomic — a single claim spans all of it, read-back included — but
/// a *sequence* of calls is not. [`relays`](Board::relays) followed by
/// [`set_relays`](Board::set_relays) is two claims, so on a board shared with
/// another application a write can land in the gap and be silently lost:
///
/// ```no_run
/// # let usb = arb::Usb::new().unwrap();
/// # let board = usb.board(None);
/// // Racy on a shared board: a write between these two calls is overwritten.
/// let active = board.relays().unwrap();
/// board.set_relays(active | arb::Relay::Three, arb::Verify::Enabled).unwrap();
/// ```
///
/// The board latches all eight relays at once, so there is no partial update to
/// reach for instead. Where the hardware is shared and read-modify-write is
/// unavoidable, coordinate outside this library.
#[derive(Clone, Debug)]
pub struct Board {
    usb: Usb,
    select: Select,
}

impl Board {
    /// Returns the USB port this board is named by, if it names one.
    ///
    /// A label, not an identifier: boards from [`Usb::boards`] always have one, but
    /// feeding it back to [`Usb::board`] can be ambiguous where the enumerated board
    /// is not. `None` only for `usb.board(None)`, which names no particular board.
    /// Use [`Display`](fmt::Display) to tell two boards apart.
    pub fn port(&self) -> Option<u8> {
        self.select.port()
    }

    /// Returns the relays that are currently active.
    ///
    /// Takes the shift register at its word: [`Board::self_test`] is the separate
    /// check that the board is still answering correctly. That check returns this
    /// same value, so there is nothing to gain by following one with this call: it
    /// would be a second claim, 28 more transfers, and on a shared board a write
    /// can land in the gap and make the two disagree.
    ///
    /// # Errors
    ///
    /// * [`Error::NotFound`] — no relay board detected
    /// * [`Error::MultipleFound`] — more than one board answers to this one
    /// * [`Error::Busy`] — another application is talking to the board
    /// * [`Error::RegisterOutOfSync`] — the read was interrupted, or the write that
    ///   puts back what it consumed was, so reading again is not the remedy
    ///
    /// # Example
    ///
    /// ```no_run
    /// let usb = arb::Usb::new().unwrap();
    ///
    /// let relays = usb.board(None).relays().unwrap();
    ///
    /// if relays.contains(arb::Relay::Three) {
    ///     println!("relay 3 is active");
    /// }
    /// ```
    pub fn relays(&self) -> Result<Relays> {
        A6275::new(self.claim()?).status().map(Relays::from_bits)
    }

    /// Checks that the board answers correctly, without moving any relay, and
    /// returns the relays it found active.
    ///
    /// Writes an inverted test pattern to the shift register and reads it back. The
    /// pattern is never latched and the register's original contents are put back
    /// afterwards, so this is safe to call on a board driving live outputs. Those
    /// contents are what it returns: the check has to read them before it can write
    /// anything, so a caller that wants both a verdict and a state gets them from
    /// one claim rather than following this with [`relays`](Board::relays).
    ///
    /// A diagnostic, not a guard on the operating path. [`Board::set_relays`] with
    /// [`Verify::Enabled`] already writes, latches, reads back and compares within
    /// a single claim — that covers everything this covers, on the value the caller
    /// actually asked for, plus the latch this deliberately never touches. And
    /// because a `self_test` is its own claim, it vouches for no other
    /// [`relays`](Board::relays) call, only for the state it hands back itself.
    ///
    /// So reach for it where a person or a monitor is asking "is this board still
    /// healthy?" — at startup, from a health check, or when a board is suspect —
    /// and not on the path of ordinary reads and writes. As a periodic probe it
    /// beats a periodic read, because it knows what answer it should get; that is
    /// early warning, not correctness.
    ///
    /// Roughly doubles the cost of a read, which is why it is not part of
    /// [`Board::relays`].
    ///
    /// # Errors
    ///
    /// * [`Error::NotFound`] — no relay board detected
    /// * [`Error::MultipleFound`] — more than one board answers to this one
    /// * [`Error::Busy`] — another application is talking to the board
    /// * [`Error::SelfTestFailed`] — the test pattern did not survive the round trip
    /// * [`Error::RegisterOutOfSync`] — the check was interrupted and could not put
    ///   the register's contents back
    pub fn self_test(&self) -> Result<Relays> {
        A6275::new(self.claim()?).self_test().map(Relays::from_bits)
    }

    /// Activates `relays`, deactivating every relay not in the set.
    ///
    /// # Arguments
    ///
    /// * `relays` — the relays to activate. [`Relays::NONE`] turns everything off.
    /// * `verify` — whether to read the shift register back after latching and return
    ///   [`Error::VerificationFailed`] on mismatch.
    ///
    /// # Errors
    ///
    /// * [`Error::NotFound`] — no relay board detected
    /// * [`Error::MultipleFound`] — more than one board answers to this one
    /// * [`Error::Busy`] — another application is talking to the board
    /// * [`Error::VerificationFailed`] — the read-back did not match `relays`
    /// * [`Error::RegisterOutOfSync`] — the read-back was interrupted and could not
    ///   put the latched value back into the register
    ///
    /// # Example
    ///
    /// ```no_run
    /// use arb::{Relay, Usb, Verify};
    ///
    /// let usb = Usb::new().unwrap();
    ///
    /// // Activate relays 1, 2, 4, 5 and 6
    /// let relays = Relay::One | Relay::Two | Relay::Four | Relay::Five | Relay::Six;
    ///
    /// usb.board(None).set_relays(relays, Verify::Enabled).unwrap();
    /// ```
    pub fn set_relays(&self, relays: Relays, verify: Verify) -> Result<()> {
        A6275::new(self.claim()?).set_status(relays.bits(), verify)
    }

    /// Performs a USB reset on the relay board.
    ///
    /// This resets the USB device, not the relays: the outputs are not changed.
    ///
    /// # Errors
    ///
    /// * [`Error::NotFound`] — no relay board detected
    /// * [`Error::MultipleFound`] — more than one board answers to this one
    /// * [`Error::Busy`] — another application is talking to the board
    pub fn reset_device(&self) -> Result<()> {
        self.claim()?.reset()
    }

    /// Finds the board and claims its CH341A interface for the duration of one call.
    fn claim(&self) -> Result<Ch341a> {
        Ch341a::open(&find_device(&self.usb.0, &self.select)?)
    }
}

/// Names which board this is: `port 3 (1-1.3)` for one from [`Usb::boards`], which
/// tells apart two boards sharing a port number, `port 3` for `usb.board(Some(3))`,
/// and `any board` for `usb.board(None)`.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.select.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_can_be_shared_across_threads() {
        // Consumers hold one `Usb` for the whole process and a `Board` per worker,
        // calling them from any thread. Interior mutability in either type — a
        // `Cell` timeout, say — would take `Sync` away and force locking on the
        // caller, serialising every board behind one lock.
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<Usb>();
        assert_send_sync::<Board>();
    }
}
