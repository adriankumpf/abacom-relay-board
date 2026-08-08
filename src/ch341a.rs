//! Low-level CH341A USB interface.
//!
//! The CH341A operates in parallel/GPIO mode. Communication uses two bulk endpoints:
//! - `ENDPOINT_OUT` (0x02): host-to-device commands
//! - `ENDPOINT_IN` (0x82): device-to-host responses
//!
//! Two commands are used:
//! - `0xA1` (set output): sets the state of the D0–D7 GPIO lines, one transfer per
//!   line change
//! - `0xAB` (UIO stream): runs a short program of pin states, so that reading a
//!   whole shift register — sample, clock, sample, … — costs one transfer out and
//!   one back rather than one transfer per state

use std::time::Duration;

use crate::errors::{Error, Result};

/// USB vendor ID for the WCH CH341A chip.
const VENDOR_ID: u16 = 0x1a86;
/// USB product ID for the CH341A in parallel/GPIO mode.
const PRODUCT_ID: u16 = 0x5512;

const ENDPOINT_OUT: u8 = 0x02;
const ENDPOINT_IN: u8 = 0x82;
/// The interface carrying the two bulk endpoints.
const INTERFACE: u8 = 0;
/// Deadlines for a single bulk transfer.
///
/// Nothing retries behind these, so a tight deadline turns a slow round trip —
/// a loaded host, or a hub between us and the board — into a hard failure
/// without buying anything. flashrom drives the same chip through one 1000 ms
/// timeout on both of its endpoints: "1000 ms is plenty and we have no backup
/// strategy anyway".
const TIMEOUT_WRITE: Duration = Duration::from_millis(1000);
const TIMEOUT_READ: Duration = Duration::from_millis(1000);

// The UIO stream command and the states that may appear in one. A stream is
// `CMD_UIO_STREAM`, a sequence of states and `UIO_STM_END`, all in a single
// packet: `OUT` drives the D0–D5 lines, `DIR` claims them as outputs, and `IN`
// samples D0–D7 and appends one byte to the stream's response.
const CMD_UIO_STREAM: u8 = 0xAB;
const UIO_STM_IN: u8 = 0x00;
const UIO_STM_END: u8 = 0x20;
const UIO_STM_DIR: u8 = 0x40;
const UIO_STM_OUT: u8 = 0x80;

/// The D0–D5 lines that can be driven, as a direction mask.
const OUTPUT_LINES: u8 = 0x3f;

/// The CH341A's packet size. A UIO stream has to fit in one.
const PACKET_LENGTH: usize = 0x20;

/// The most samples one stream can take: three states each, after the command,
/// the initial `OUT` and the terminator.
const MAX_SAMPLES: usize = (PACKET_LENGTH - 3) / 3;

pub type Device = rusb::Device<rusb::Context>;
type DeviceHandle = rusb::DeviceHandle<rusb::Context>;

fn expect_transfer_len(actual: usize, expected: usize) -> Result<()> {
    if actual == expected {
        return Ok(());
    }

    Err(Error::UnexpectedTransferLength { expected, actual })
}

/// Sends `msg` to the device, failing if it was not transferred whole.
fn write_msg(handle: &DeviceHandle, msg: &[u8]) -> Result<()> {
    let written = handle.write_bulk(ENDPOINT_OUT, msg, TIMEOUT_WRITE)?;

    expect_transfer_len(written, msg.len())
}

/// Encodes the UIO stream behind [`Gpio::sample_clocked`].
///
/// Returns the packet and the number of bytes used.
fn sample_stream(clock: u8, samples: usize) -> ([u8; PACKET_LENGTH], usize) {
    let mut packet = [0u8; PACKET_LENGTH];
    let end = 2 + samples * 3;

    packet[0] = CMD_UIO_STREAM;
    packet[1] = UIO_STM_OUT; // every line low, `clock` included

    for states in packet[2..end].chunks_exact_mut(3) {
        states.copy_from_slice(&[UIO_STM_IN, UIO_STM_OUT | clock, UIO_STM_OUT]);
    }

    packet[end] = UIO_STM_END;

    (packet, end + 1)
}

/// Returns whether `device` is a CH341A in parallel/GPIO mode.
pub fn is_ch341a(device: &Device) -> Result<bool> {
    let dd = device.device_descriptor()?;

    Ok(dd.vendor_id() == VENDOR_ID && dd.product_id() == PRODUCT_ID)
}

/// The D0–D7 GPIO lines the A6275 is driven through.
///
/// Setting and reading those lines is all the relay board needs from the CH341A,
/// so keeping it behind a trait lets the shift register protocol be exercised
/// against a simulated A6275 instead of real hardware.
pub trait Gpio {
    /// Sets the D0–D7 output lines to `data`.
    ///
    /// Each bit in `data` corresponds to one GPIO line. On the ABACOM relay board:
    /// - Bit 0 (0x01): A6275 LATCH
    /// - Bit 3 (0x08): A6275 CLK
    /// - Bit 5 (0x20): A6275 Serial DATA in
    fn set_output(&self, data: u8) -> Result<()>;

    /// Takes `N` readings of the D0–D7 input lines, one per pulse of `clock`.
    ///
    /// Each reading is taken before `clock` goes high, so a device that shifts on
    /// the rising edge is sampled once per bit, first bit first. Every line other
    /// than `clock` is held low throughout, and all of them are left low.
    ///
    /// On the ABACOM relay board, bit 7 (D7) of each reading carries the A6275
    /// serial output, which is how the shift register is read back.
    fn sample_clocked<const N: usize>(&self, clock: u8) -> Result<[u8; N]>;
}

/// An opened CH341A with its bulk interface claimed.
pub struct Ch341a {
    handle: DeviceHandle,
}

impl Ch341a {
    /// Opens `device` and claims its bulk interface.
    ///
    /// Any kernel driver bound to the interface is detached when the interface
    /// is claimed and re-attached when it is released, so the board is left as
    /// it was found. Platforms whose libusb lacks that capability report
    /// `NotSupported` and are ignored: rusb then behaves as if the call had
    /// never been made, and there is no kernel driver to detach there anyway.
    ///
    /// The claim is exclusive, so opening a board another application is currently
    /// talking to fails with [`Error::Busy`].
    pub fn open(device: &Device) -> Result<Self> {
        let handle = device.open()?;

        match handle.set_auto_detach_kernel_driver(true) {
            Ok(()) | Err(rusb::Error::NotSupported) => {}
            Err(e) => return Err(e.into()),
        }

        // `Busy` here means the interface is already claimed, which is contention
        // rather than a USB fault, so it is reported as its own error.
        handle.claim_interface(INTERFACE).map_err(|e| match e {
            rusb::Error::Busy => Error::Busy,
            e => e.into(),
        })?;

        // The `0xA1` write path carries the line directions in every message, but a
        // UIO stream carries none, so they are claimed once here — as flashrom does
        // for the same chip. The two must keep saying the same thing.
        write_msg(
            &handle,
            &[CMD_UIO_STREAM, UIO_STM_DIR | OUTPUT_LINES, UIO_STM_END],
        )?;

        Ok(Self { handle })
    }

    /// Performs a USB port reset on the device.
    pub fn reset(&self) -> Result<()> {
        Ok(self.handle.reset()?)
    }
}

impl Gpio for Ch341a {
    fn set_output(&self, data: u8) -> Result<()> {
        #[rustfmt::skip]
        let msg = [
            0xA1, 0x6a, 0x1f, 0x00, 0x10, data, OUTPUT_LINES, 0x00, 0x00, 0x00, 0x00,
        ];

        write_msg(&self.handle, &msg)
    }

    fn sample_clocked<const N: usize>(&self, clock: u8) -> Result<[u8; N]> {
        const { assert!(N <= MAX_SAMPLES, "a UIO stream must fit one packet") };

        let (packet, len) = sample_stream(clock, N);
        write_msg(&self.handle, &packet[..len])?;

        // One byte per `UIO_STM_IN`, in the order the stream ran them.
        let mut samples = [0u8; N];
        let read = self
            .handle
            .read_bulk(ENDPOINT_IN, &mut samples, TIMEOUT_READ)?;
        expect_transfer_len(read, N)?;

        Ok(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock line. Which one is the protocol layer's business, not this module's.
    const CLOCK: u8 = 0x08;

    #[test]
    fn a_sample_stream_reads_before_every_rising_clock_edge() {
        let (packet, len) = sample_stream(CLOCK, 2);

        assert_eq!(
            &packet[..len],
            &[
                0xAB, // UIO stream
                0x80, // every line low
                0x00, 0x88, 0x80, // read, clock high, clock low
                0x00, 0x88, 0x80, // read, clock high, clock low
                0x20, // end
            ]
        );
    }

    #[test]
    fn the_longest_stream_fits_one_packet() {
        let (_, len) = sample_stream(CLOCK, MAX_SAMPLES);

        assert!(len <= PACKET_LENGTH, "{len} bytes exceeds {PACKET_LENGTH}");
    }

    #[test]
    fn expect_transfer_len_accepts_exact_length() {
        assert!(expect_transfer_len(6, 6).is_ok());
    }

    #[test]
    fn expect_transfer_len_rejects_short_transfers() {
        assert!(matches!(
            expect_transfer_len(5, 6),
            Err(Error::UnexpectedTransferLength {
                expected: 6,
                actual: 5
            })
        ));
    }

    #[test]
    fn expect_transfer_len_rejects_long_transfers() {
        assert!(matches!(
            expect_transfer_len(7, 6),
            Err(Error::UnexpectedTransferLength {
                expected: 6,
                actual: 7
            })
        ));
    }
}
