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
//! # Examples
//!
//! ```no_run
//! use arb::{Relay, Relays, Usb, Verify};
//!
//! // Initialise libusb once and keep it: this is the expensive part
//! let usb = Usb::new().unwrap();
//! let board = usb.board(None);
//!
//! // Confirm the board is answering — worth doing once, not on every read
//! board.self_test().unwrap();
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

mod ch341a;
mod errors;
mod find;
mod relays;

use self::ch341a::{Ch341a, Gpio};
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

// Allegro A6275 pin mapping on the CH341A D0–D7 GPIO lines.
const LATCH: u8 = 0x01; // D0 → A6275 Latch
const CLK: u8 = 0x08; // D3 → A6275 CLK
const DATA: u8 = 0x20; // D5 → A6275 Serial in
const READ: u8 = 0x80; // D7 ← A6275 Serial out

/// The Allegro A6275 shift register protocol, driven over a set of GPIO lines.
struct A6275<T> {
    gpio: T,
}

impl<T: Gpio> A6275<T> {
    fn new(gpio: T) -> Self {
        Self { gpio }
    }

    /// Shifts 8 bits into the A6275 shift register (MSB first) without latching.
    ///
    /// Leaves all output lines low.
    fn shift_out_bits(&self, status: u8) -> Result<()> {
        self.gpio.set_output(0)?;

        for bit in (0..8).rev() {
            let data = if status & (1 << bit) != 0 { DATA } else { 0 };

            self.gpio.set_output(data)?;
            self.gpio.set_output(CLK | data)?;
            self.gpio.set_output(data)?;
        }

        self.gpio.set_output(0)
    }

    /// Clocks the 8 bits of the A6275 shift register out of its serial output (D7).
    ///
    /// One clocked read, so two USB transfers rather than one per line change.
    ///
    /// Destructive: reading shifts zeros in, so the caller must restore the register
    /// with [`A6275::shift_out_bits`] if its contents still matter.
    fn read_shift_register(&self) -> Result<u8> {
        let mut status = 0;

        // The register presents its most significant bit first.
        for sample in self.gpio.sample_clocked::<8>(CLK)? {
            status = status << 1 | u8::from(sample & READ != 0);
        }

        Ok(status)
    }

    /// Shifts `status` into the A6275 and latches it to the relay outputs.
    ///
    /// If `verify` is [`Verify::Enabled`], reads back the shift register and returns
    /// [`Error::VerificationFailed`] if it doesn't match.
    fn set_status(&self, status: u8, verify: Verify) -> Result<()> {
        self.shift_out_bits(status)?;

        self.gpio.set_output(LATCH)?;
        self.gpio.set_output(0)?;

        if verify == Verify::Enabled {
            let read = self.status()?;

            if read != status {
                return Err(Error::VerificationFailed {
                    expected: Relays::from_bits(status),
                    actual: Relays::from_bits(read),
                });
            }
        }

        Ok(())
    }

    /// Reads the shift register and puts back what reading it consumed.
    fn status(&self) -> Result<u8> {
        let status = self.read_shift_register()?;

        self.shift_out_bits(status)?;

        Ok(status)
    }

    /// Checks that a test pattern survives the round trip through the shift register.
    ///
    /// Writes the complement of the register's current contents without latching, so
    /// the relay outputs are never touched, reads it back and puts the original
    /// contents back — including when the check fails, so that a failure does not
    /// leave the register disagreeing with the latched outputs.
    fn self_test(&self) -> Result<()> {
        let status = self.read_shift_register()?;
        let test_status = !status;

        self.shift_out_bits(test_status)?;
        let read = self.read_shift_register()?;
        self.shift_out_bits(status)?;

        if read != test_status {
            return Err(Error::SelfTestFailed);
        }

        Ok(())
    }
}

/// A libusb context: how relay boards are found.
///
/// Initialising it is by far the most expensive part of talking to a board, so
/// create one and keep it. It is cheap to clone (reference-counted) and safe to
/// share across threads. It claims nothing and opens nothing, so contexts never
/// conflict with each other or with another application using the same board.
///
/// Not self-healing: if the USB controller resets or the host suspends, a `Usb`
/// can go permanently sour. Callers that must survive that should drop it and
/// build a new one after repeated failures.
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
    /// check that the board is still answering correctly.
    ///
    /// # Errors
    ///
    /// * [`Error::NotFound`] — no relay board detected
    /// * [`Error::MultipleFound`] — multiple boards detected and no port was given
    /// * [`Error::Busy`] — another application is talking to the board
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

    /// Checks that the board answers correctly, without moving any relay.
    ///
    /// Writes an inverted test pattern to the shift register and reads it back. The
    /// pattern is never latched and the register's original contents are put back
    /// afterwards, so this is safe to call on a board driving live outputs.
    ///
    /// Roughly doubles the cost of a read, which is why it is not part of
    /// [`Board::relays`]: call it when a board is suspect, or periodically, rather
    /// than on every read.
    ///
    /// # Errors
    ///
    /// * [`Error::NotFound`] — no relay board detected
    /// * [`Error::MultipleFound`] — multiple boards detected and no port was given
    /// * [`Error::Busy`] — another application is talking to the board
    /// * [`Error::SelfTestFailed`] — the test pattern did not survive the round trip
    pub fn self_test(&self) -> Result<()> {
        A6275::new(self.claim()?).self_test()
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
    /// * [`Error::MultipleFound`] — multiple boards detected and no port was given
    /// * [`Error::Busy`] — another application is talking to the board
    /// * [`Error::VerificationFailed`] — the read-back did not match `relays`
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
    /// * [`Error::MultipleFound`] — multiple boards detected and no port was given
    /// * [`Error::Busy`] — another application is talking to the board
    pub fn reset_device(&self) -> Result<()> {
        self.claim()?.reset()
    }

    /// Finds the board and claims its CH341A interface for the duration of one call.
    fn claim(&self) -> Result<Ch341a> {
        Ch341a::open(&find_device(&self.usb.0, &self.select)?)
    }
}

/// Names which board this is: `port 3 (bus 1, path 1.3)` for one from
/// [`Usb::boards`], which tells apart two boards sharing a port number, `port 3` for
/// `usb.board(Some(3))`, and `any board` for `usb.board(None)`.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.select.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// The register bit presented on the A6275 serial output.
    const MSB: u8 = 0b1000_0000;

    /// A simulated A6275 driven through the same GPIO lines as the real one.
    ///
    /// The register shifts left on each rising CLK edge, taking in the DATA line,
    /// and presents its most significant bit on the serial output (D7). A rising
    /// LATCH edge copies the register to the relay outputs.
    #[derive(Default)]
    struct FakeA6275 {
        lines: Cell<u8>,
        register: Cell<u8>,
        outputs: Cell<u8>,
    }

    impl Gpio for FakeA6275 {
        fn set_output(&self, data: u8) -> Result<()> {
            let previous = self.lines.replace(data);
            let rising = |pin: u8| previous & pin == 0 && data & pin != 0;

            if rising(CLK) {
                let bit = u8::from(data & DATA != 0);
                self.register.set(self.register.get() << 1 | bit);
            }

            if rising(LATCH) {
                self.outputs.set(self.register.get());
            }

            Ok(())
        }

        /// Drives the sampling one state at a time, exactly as the pin states a
        /// UIO stream runs would: batching it on the wire must not change what the
        /// device sees.
        fn sample_clocked<const N: usize>(&self, clock: u8) -> Result<[u8; N]> {
            let mut samples = [0; N];

            self.set_output(0)?;

            for sample in &mut samples {
                let serial_out = self.register.get() & MSB != 0;

                *sample = if serial_out { READ } else { 0 };

                self.set_output(clock)?;
                self.set_output(0)?;
            }

            Ok(samples)
        }
    }

    /// A device that never drives its serial output, so every read-back mismatches.
    struct StuckLow;

    impl Gpio for StuckLow {
        fn set_output(&self, _data: u8) -> Result<()> {
            Ok(())
        }

        fn sample_clocked<const N: usize>(&self, _clock: u8) -> Result<[u8; N]> {
            Ok([0; N])
        }
    }

    /// A board that drops the most significant bit of every read — the first bit
    /// the register presents — so a read-back disagrees with what was written while
    /// the register underneath stays real, which is what tells a failed check from
    /// an untouched board.
    struct FlakyRead(FakeA6275);

    impl Gpio for FlakyRead {
        fn set_output(&self, data: u8) -> Result<()> {
            self.0.set_output(data)
        }

        fn sample_clocked<const N: usize>(&self, clock: u8) -> Result<[u8; N]> {
            let mut samples = self.0.sample_clocked(clock)?;

            if let Some(first) = samples.first_mut() {
                *first &= !READ;
            }

            Ok(samples)
        }
    }

    /// Counts USB transfers on their way to a simulated board: one for a line
    /// change, and one out plus one back for a clocked read.
    struct Counting {
        gpio: FakeA6275,
        transfers: Cell<usize>,
    }

    impl Gpio for Counting {
        fn set_output(&self, data: u8) -> Result<()> {
            self.transfers.set(self.transfers.get() + 1);

            self.gpio.set_output(data)
        }

        fn sample_clocked<const N: usize>(&self, clock: u8) -> Result<[u8; N]> {
            self.transfers.set(self.transfers.get() + 2);

            self.gpio.sample_clocked(clock)
        }
    }

    fn fake() -> A6275<FakeA6275> {
        A6275::new(FakeA6275::default())
    }

    fn flaky() -> A6275<FlakyRead> {
        A6275::new(FlakyRead(FakeA6275::default()))
    }

    /// Runs `call` against a counting board and returns the transfers it cost.
    fn transfers<R>(call: impl FnOnce(&A6275<Counting>) -> Result<R>) -> usize {
        let board = A6275::new(Counting {
            gpio: FakeA6275::default(),
            transfers: Cell::new(0),
        });

        call(&board).unwrap();

        board.gpio.transfers.get()
    }

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

    #[test]
    fn shift_out_then_read_round_trips_every_value() {
        for status in 0..=u8::MAX {
            let board = fake();

            board.shift_out_bits(status).unwrap();

            assert_eq!(board.read_shift_register().unwrap(), status);
        }
    }

    #[test]
    fn bits_are_shifted_out_most_significant_first() {
        // Pins the direction directly at the register, without routing through
        // `read_shift_register`: clocking `0b1000_0000` out least-significant-first
        // would leave it in the register as `0b0000_0001`.
        let board = fake();

        board.shift_out_bits(0b1000_0000).unwrap();

        assert_eq!(board.gpio.register.get(), 0b1000_0000);
    }

    #[test]
    fn reading_the_register_leaves_it_empty() {
        let board = fake();
        board.shift_out_bits(0b1011_0010).unwrap();

        board.read_shift_register().unwrap();

        assert_eq!(board.gpio.register.get(), 0);
    }

    #[test]
    fn shifting_bits_out_never_latches_the_outputs() {
        let board = fake();
        board.gpio.outputs.set(0b1010_1010);

        board.shift_out_bits(0b0101_0101).unwrap();

        assert_eq!(board.gpio.outputs.get(), 0b1010_1010);
    }

    #[test]
    fn set_status_latches_the_requested_relays() {
        let board = fake();

        board.set_status(0b0011_0111, Verify::Enabled).unwrap();

        assert_eq!(board.gpio.outputs.get(), 0b0011_0111);
        // Verification reads the register destructively, so it has to be restored.
        assert_eq!(board.gpio.register.get(), 0b0011_0111);
    }

    #[test]
    fn set_status_reports_a_read_back_mismatch() {
        // An asymmetric pattern, so that reporting the two sets the wrong way round
        // is caught rather than looking identical.
        let err = A6275::new(StuckLow)
            .set_status(0b1010_0110, Verify::Enabled)
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "verification failed: expected 2 3 6 8, read back none"
        );

        let Error::VerificationFailed { expected, actual } = err else {
            panic!("expected a verification failure, got {err:?}");
        };

        assert_eq!(expected, Relays::from_bits(0b1010_0110));
        assert_eq!(actual, Relays::NONE);
    }

    #[test]
    fn set_status_skips_the_read_back_when_not_verifying() {
        assert!(
            A6275::new(StuckLow)
                .set_status(0b0000_0001, Verify::Disabled)
                .is_ok()
        );
    }

    #[test]
    fn status_returns_the_latched_relays() {
        let board = fake();
        board.set_status(0b0011_0111, Verify::Disabled).unwrap();

        assert_eq!(board.status().unwrap(), 0b0011_0111);
    }

    #[test]
    fn status_puts_back_the_register_it_consumed() {
        let board = fake();
        board.set_status(0b1100_1001, Verify::Disabled).unwrap();

        board.status().unwrap();

        // Reading shifts zeros in, so a plain read still has to write the register
        // back; leaving it empty would make the next read report no relays at all.
        assert_eq!(board.gpio.register.get(), 0b1100_1001);
        assert_eq!(board.gpio.outputs.get(), 0b1100_1001);
    }

    #[test]
    fn the_self_test_leaves_the_board_as_it_found_it() {
        let board = fake();
        board.set_status(0b1100_1001, Verify::Disabled).unwrap();

        board.self_test().unwrap();

        // The check must neither latch its test pattern nor consume the register
        // contents it read.
        assert_eq!(board.gpio.outputs.get(), 0b1100_1001);
        assert_eq!(board.gpio.register.get(), 0b1100_1001);
    }

    #[test]
    fn the_self_test_reports_an_unresponsive_device() {
        let err = A6275::new(StuckLow).self_test().unwrap_err();

        assert!(matches!(err, Error::SelfTestFailed));
    }

    #[test]
    fn a_failed_self_test_still_puts_the_register_back() {
        let board = flaky();
        board.set_status(0b0011_0101, Verify::Disabled).unwrap();

        board.self_test().unwrap_err();

        // Bailing out before the restore would leave the register holding the zeros
        // the read shifted in, so the next read would disagree with the latched
        // outputs without any relay having moved.
        assert_eq!(board.gpio.0.register.get(), 0b0011_0101);
        assert_eq!(board.gpio.0.outputs.get(), 0b0011_0101);
    }

    #[test]
    fn a_board_that_fails_the_self_test_still_reports_its_state() {
        // Reading and checking are separate questions: a plain read takes the
        // register at its word. The pattern's top bit is clear, so it survives a
        // read that drops that bit while the inverted pattern the check writes
        // does not.
        let board = flaky();
        board.set_status(0b0011_0101, Verify::Disabled).unwrap();

        assert_eq!(board.status().unwrap(), 0b0011_0101);
        assert!(board.self_test().is_err());
    }

    #[test]
    fn the_protocol_costs_the_transfers_it_should() {
        // The point of the clocked read: 33 transfers as one write and one read per
        // bit, 2 as a single stream. At the measured ~41 µs per transfer that is
        // 1.4 ms against 0.1 ms, and it is why `status` below is 28 and not 59.
        assert_eq!(transfers(|board| board.read_shift_register()), 2);

        // Writing is still one transfer per line change: 8 bits × 3 states, plus the
        // low state either side. It stays that way deliberately: the CH341A emits
        // stream states faster than the DATA line settles, so a batched write clocks
        // in the previous bit.
        assert_eq!(transfers(|board| board.shift_out_bits(0b1010_1010)), 26);

        // Plus the two states that latch the outputs.
        assert_eq!(transfers(|board| board.set_status(0, Verify::Disabled)), 28);

        // Verifying adds a read and the restore that a destructive read costs.
        assert_eq!(transfers(|board| board.set_status(0, Verify::Enabled)), 56);

        // A plain read is that same read and restore, and nothing else.
        assert_eq!(transfers(|board| board.status()), 28);

        // The self-test is what doubles it: read, write the test pattern, read it
        // back, put the original contents back. Splitting it out of the read is
        // worth those 28 transfers — about 1.1 ms — on every call that only reads.
        assert_eq!(transfers(|board| board.self_test()), 56);
    }
}
