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

use rusb::UsbContext;

mod ch341a;
mod errors;
mod relays;

use self::ch341a::{Ch341a, Gpio};

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
    /// Destructive: reading shifts zeros in, so the caller must restore the register
    /// with [`A6275::shift_out_bits`] if its contents still matter.
    fn read_shift_register(&self) -> Result<u8> {
        let mut status = 0;

        self.gpio.set_output(0)?;

        for bit in (0..8).rev() {
            if self.gpio.get_input()? & READ != 0 {
                status |= 1 << bit;
            }

            self.gpio.set_output(CLK)?;
            self.gpio.set_output(0)?;
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
            let read = self.read_shift_register()?;
            self.shift_out_bits(read)?;

            if read != status {
                return Err(Error::VerificationFailed);
            }
        }

        Ok(())
    }

    /// Reads the current relay state, checking that the device is responsive.
    fn get_status(&self) -> Result<u8> {
        let status = self.read_shift_register()?;
        let test_status = !status;

        // Health check: write an inverted test pattern to the shift register without
        // latching, so the relay outputs are left untouched, and read it back.
        self.shift_out_bits(test_status)?;

        if self.read_shift_register()? != test_status {
            return Err(Error::BadDevice);
        }

        self.shift_out_bits(status)?;

        Ok(status)
    }
}

fn find_device(context: &rusb::Context, port: Option<u8>) -> Result<ch341a::Device> {
    let mut found = None;

    for device in context.devices()?.iter() {
        if ch341a::is_ch341a(&device)? && port.is_none_or(|p| device.port_number() == p) {
            if found.is_some() {
                return Err(Error::MultipleFound);
            }

            found = Some(device);
        }
    }

    found.ok_or(Error::NotFound)
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
    /// # Arguments
    ///
    /// * `port` — USB port number to select a specific board when multiple are connected.
    pub fn board(&self, port: Option<u8>) -> Board {
        Board {
            usb: self.clone(),
            port,
        }
    }
}

/// One relay board, found and claimed afresh for the duration of every call.
///
/// Holds no device and no claim between calls, so several `Board`s — in this
/// process or in another application — can drive the same hardware.
#[derive(Clone, Debug)]
pub struct Board {
    usb: Usb,
    port: Option<u8>,
}

impl Board {
    /// Returns the relays that are currently active.
    ///
    /// Internally verifies the device is responsive by writing an inverted test pattern to the
    /// shift register (without latching, so relay outputs are not disturbed) and reading it back.
    /// Returns [`Error::BadDevice`] if the read-back doesn't match.
    ///
    /// # Errors
    ///
    /// * [`Error::NotFound`] — no relay board detected
    /// * [`Error::MultipleFound`] — multiple boards detected and no port was given
    /// * [`Error::BadDevice`] — device did not respond correctly to the read-back test
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
        A6275::new(self.claim()?)
            .get_status()
            .map(Relays::from_bits)
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
    pub fn reset_device(&self) -> Result<()> {
        self.claim()?.reset()
    }

    /// Finds the board and claims its CH341A interface for the duration of one call.
    fn claim(&self) -> Result<Ch341a> {
        Ch341a::open(&find_device(&self.usb.0, self.port)?)
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

        fn get_input(&self) -> Result<u8> {
            let serial_out = self.register.get() & MSB != 0;

            Ok(if serial_out { READ } else { 0 })
        }
    }

    /// A device that never drives its serial output, so every read-back mismatches.
    struct StuckLow;

    impl Gpio for StuckLow {
        fn set_output(&self, _data: u8) -> Result<()> {
            Ok(())
        }

        fn get_input(&self) -> Result<u8> {
            Ok(0)
        }
    }

    fn fake() -> A6275<FakeA6275> {
        A6275::new(FakeA6275::default())
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
        let err = A6275::new(StuckLow)
            .set_status(0b0000_0001, Verify::Enabled)
            .unwrap_err();

        assert!(matches!(err, Error::VerificationFailed));
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
    fn get_status_returns_the_latched_relays() {
        let board = fake();
        board.set_status(0b0011_0111, Verify::Disabled).unwrap();

        assert_eq!(board.get_status().unwrap(), 0b0011_0111);
    }

    #[test]
    fn get_status_leaves_the_board_as_it_found_it() {
        let board = fake();
        board.set_status(0b1100_1001, Verify::Disabled).unwrap();

        board.get_status().unwrap();

        // The health check must neither latch its test pattern nor consume the
        // register contents it read.
        assert_eq!(board.gpio.outputs.get(), 0b1100_1001);
        assert_eq!(board.gpio.register.get(), 0b1100_1001);
    }

    #[test]
    fn get_status_reports_an_unresponsive_device() {
        let err = A6275::new(StuckLow).get_status().unwrap_err();

        assert!(matches!(err, Error::BadDevice));
    }
}
