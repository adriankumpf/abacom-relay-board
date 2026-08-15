//! The Allegro A6275 shift register protocol.
//!
//! The board's eight relays hang off an A6275, which the CH341A drives as an
//! SPI-like slave: bits are clocked in serially on DATA, one per rising CLK edge,
//! and a rising LATCH edge copies the register to the relay outputs. The register
//! also clocks back out of its serial output, which is how the state is read.
//!
//! [`A6275`] is generic over [`Gpio`] rather than talking to a `Ch341a` directly,
//! which is what lets every path here be tested against a simulated shift register
//! instead of hardware.

use crate::Verify;
use crate::ch341a::{Gpio, SAMPLES};
use crate::errors::{Error, Result};
use crate::relays::Relays;

// Allegro A6275 pin mapping on the CH341A D0–D7 GPIO lines.
const LATCH: u8 = 0x01; // D0 → A6275 Latch
const CLK: u8 = 0x08; // D3 → A6275 CLK
const DATA: u8 = 0x20; // D5 → A6275 Serial in
const READ: u8 = 0x80; // D7 ← A6275 Serial out

/// The Allegro A6275 shift register protocol, driven over a set of GPIO lines.
pub struct A6275<T> {
    gpio: T,
}

impl<T: Gpio> A6275<T> {
    pub fn new(gpio: T) -> Self {
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
        // What ties the wire layer's sample count to this register: one sample per
        // bit, folded into the `u8` below, which a wider read would silently truncate.
        const { assert!(SAMPLES == u8::BITS as usize, "the register is one byte") };

        let mut status = 0;

        // The register presents its most significant bit first.
        for sample in self.gpio.sample_clocked(CLK)? {
            status = status << 1 | u8::from(sample & READ != 0);
        }

        Ok(status)
    }

    /// Puts `status` back into the register after a read consumed it.
    fn restore(&self, status: u8) -> Result<()> {
        self.shift_out_bits(status).map_err(Error::out_of_sync)
    }

    /// Runs `op`, then puts `status` back into the register whether it succeeded or not.
    ///
    /// Bailing out of a destructive read would leave the register holding the zeros
    /// the read shifted in, so the next read would report relays as inactive while
    /// the outputs still hold them. A failed restore outranks whatever `op` returned,
    /// because it is the one that says later reads cannot be trusted.
    fn restoring<R>(&self, status: u8, op: impl FnOnce() -> Result<R>) -> Result<R> {
        let result = op();

        self.restore(status)?;

        result
    }

    /// Shifts `status` into the A6275 and latches it to the relay outputs.
    ///
    /// If `verify` is [`Verify::Enabled`], reads back the shift register and returns
    /// [`Error::VerificationFailed`] if it doesn't match.
    pub fn set_status(&self, status: u8, verify: Verify) -> Result<()> {
        self.shift_out_bits(status)?;

        self.gpio.set_output(LATCH)?;
        self.gpio.set_output(0)?;

        if verify == Verify::Enabled {
            // The outputs hold `status` now, so that is what the register has to be
            // left holding, on a mismatch as much as on a match. Putting the
            // read-back value there instead would leave the register carrying a
            // figure that came from the very path the mismatch implicates, and the
            // next read would report relays nobody asked for.
            let read = self.restoring(status, || self.read_shift_register())?;

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
    pub fn status(&self) -> Result<u8> {
        let status = self.read_shift_register().map_err(Error::out_of_sync)?;

        self.restore(status)?;

        Ok(status)
    }

    /// Checks that a test pattern survives the round trip through the shift register.
    ///
    /// Writes the complement of the register's current contents without latching, so
    /// the relay outputs are never touched, reads it back and puts the original
    /// contents back on every path out, so that neither a failed check nor a failed
    /// transfer leaves the register disagreeing with the latched outputs.
    pub fn self_test(&self) -> Result<()> {
        let status = self.read_shift_register().map_err(Error::out_of_sync)?;
        let test_status = !status;

        let read = self.restoring(status, || {
            self.shift_out_bits(test_status)?;
            self.read_shift_register()
        })?;

        if read != test_status {
            return Err(Error::SelfTestFailed);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::error::Error as _;

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
        fn sample_clocked(&self, clock: u8) -> Result<[u8; SAMPLES]> {
            let mut samples = [0; SAMPLES];

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

        fn sample_clocked(&self, _clock: u8) -> Result<[u8; SAMPLES]> {
            Ok([0; SAMPLES])
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

        fn sample_clocked(&self, clock: u8) -> Result<[u8; SAMPLES]> {
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

        fn sample_clocked(&self, clock: u8) -> Result<[u8; SAMPLES]> {
            self.transfers.set(self.transfers.get() + 2);

            self.gpio.sample_clocked(clock)
        }
    }

    /// A board whose transport gives out part way through, so that the error paths
    /// can be driven without a flaky board to hand.
    ///
    /// The clocked read hands its failure back *after* the simulated register has
    /// been clocked out, which is the case that matters: on the wire the stream is
    /// what clocks the register, so a response that never arrives has already cost
    /// the contents.
    struct FailingAfter {
        gpio: FakeA6275,
        reads: Cell<usize>,
        writes: Cell<usize>,
    }

    impl FailingAfter {
        /// Whether `budget` has a transfer left, spending it if so.
        fn spend(budget: &Cell<usize>) -> Result<()> {
            match budget.get() {
                0 => Err(Error::Usb(rusb::Error::Timeout)),
                left => {
                    budget.set(left - 1);
                    Ok(())
                }
            }
        }
    }

    impl Gpio for FailingAfter {
        fn set_output(&self, data: u8) -> Result<()> {
            Self::spend(&self.writes)?;

            self.gpio.set_output(data)
        }

        fn sample_clocked(&self, clock: u8) -> Result<[u8; SAMPLES]> {
            let samples = self.gpio.sample_clocked(clock)?;

            Self::spend(&self.reads)?;

            Ok(samples)
        }
    }

    fn fake() -> A6275<FakeA6275> {
        A6275::new(FakeA6275::default())
    }

    fn flaky() -> A6275<FlakyRead> {
        A6275::new(FlakyRead(FakeA6275::default()))
    }

    /// A board holding `latched` in both its register and its outputs, that takes
    /// `reads` clocked reads and `writes` line changes before answering [`Error::Usb`].
    fn failing(latched: u8, reads: usize, writes: usize) -> A6275<FailingAfter> {
        let board = A6275::new(FailingAfter {
            gpio: FakeA6275::default(),
            reads: Cell::new(reads),
            writes: Cell::new(writes),
        });

        board.gpio.gpio.register.set(latched);
        board.gpio.gpio.outputs.set(latched);

        board
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
    fn a_failed_verification_leaves_the_latched_value_in_the_register() {
        let board = flaky();

        let err = board.set_status(0b1000_0001, Verify::Enabled).unwrap_err();

        assert!(matches!(err, Error::VerificationFailed { .. }));

        // The read dropped the top bit, and the outputs hold what was latched, so
        // the register has to hold that too. Leaving the read-back value there
        // would make the next read agree with the fault rather than with the
        // relays, and it is the read path the mismatch implicates.
        assert_eq!(board.gpio.0.register.get(), 0b1000_0001);
        assert_eq!(board.gpio.0.outputs.get(), 0b1000_0001);
    }

    #[test]
    fn a_read_that_never_comes_back_reports_a_lost_register() {
        let board = failing(0b0011_0101, 0, usize::MAX);

        let err = board.status().unwrap_err();

        // A plain transport error would invite a retry that succeeds and reports no
        // relays at all on a board holding five.
        assert!(matches!(err, Error::RegisterOutOfSync { .. }));
        // The transport failure stays reachable through the source chain, which is
        // what `{source}` in the message and a caller walking `Error::source` read.
        assert!(err.source().is_some());
        assert_eq!(board.gpio.gpio.register.get(), 0);
        assert_eq!(board.gpio.gpio.outputs.get(), 0b0011_0101);
    }

    #[test]
    fn a_restore_that_gives_out_part_way_reports_a_lost_register() {
        // The read lands; the line changes that put it back do not.
        let board = failing(0b1100_1001, 1, 5);

        let err = board.status().unwrap_err();

        assert!(matches!(err, Error::RegisterOutOfSync { .. }));
        assert_ne!(board.gpio.gpio.register.get(), 0b1100_1001);
    }

    #[test]
    fn a_failed_restore_outranks_the_failure_that_preceded_it() {
        // Enough writes for the test pattern but not for the restore after it, so
        // both halves fail: the read with a transport error, the restore with a lost
        // register. The restore wins, because it is the one saying later reads cannot
        // be trusted, and a plain `Timeout` here would invite the retry that sticks.
        let board = failing(0b0011_0101, 1, 26);

        let err = board.self_test().unwrap_err();

        assert!(matches!(err, Error::RegisterOutOfSync { .. }));
    }

    #[test]
    fn a_self_test_cut_short_still_puts_the_register_back() {
        // The first read lands and the second does not, so the original contents are
        // still known and the restore can run.
        let board = failing(0b0011_0101, 1, usize::MAX);

        let err = board.self_test().unwrap_err();

        // A transport error rather than a lost register, because the restore had the
        // original to hand.
        assert!(matches!(err, Error::Usb(_)));
        assert_eq!(board.gpio.gpio.register.get(), 0b0011_0101);
        assert_eq!(board.gpio.gpio.outputs.get(), 0b0011_0101);
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
