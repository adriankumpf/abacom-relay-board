//! Low-level CH341A USB interface.
//!
//! The CH341A operates in parallel/GPIO mode. Communication uses two bulk endpoints:
//! - `ENDPOINT_OUT` (0x02): host-to-device commands
//! - `ENDPOINT_IN` (0x82): device-to-host responses
//!
//! Two commands are used:
//! - `0xA1` (set output): sets the state of the D0–D7 GPIO lines
//! - `0xA0` (get input): reads back 6 bytes of pin state; only byte 0 (D0–D7) is
//!   relevant for the relay board, where D7 is wired to the A6275 serial output

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
const GET_INPUT_RESPONSE_LEN: usize = 6;

pub type Device = rusb::Device<rusb::Context>;
type DeviceHandle = rusb::DeviceHandle<rusb::Context>;

fn expect_transfer_len(actual: usize, expected: usize) -> Result<()> {
    if actual == expected {
        return Ok(());
    }

    Err(Error::UnexpectedTransferLength { expected, actual })
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

    /// Reads the D0–D7 input lines and returns byte 0 (D7–D0).
    ///
    /// On the ABACOM relay board, bit 7 (D7) carries the A6275 serial output,
    /// used to read back the current shift register contents.
    fn get_input(&self) -> Result<u8>;
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

        Ok(Self { handle })
    }

    /// Performs a USB port reset on the device.
    pub fn reset(&self) -> Result<()> {
        Ok(self.handle.reset()?)
    }
}

impl Gpio for Ch341a {
    fn set_output(&self, data: u8) -> Result<()> {
        let msg = [
            0xA1, 0x6a, 0x1f, 0x00, 0x10, data, 0x3f, 0x00, 0x00, 0x00, 0x00,
        ];
        let written = self.handle.write_bulk(ENDPOINT_OUT, &msg, TIMEOUT_WRITE)?;

        expect_transfer_len(written, msg.len())
    }

    fn get_input(&self) -> Result<u8> {
        let msg = [0xA0];
        let written = self.handle.write_bulk(ENDPOINT_OUT, &msg, TIMEOUT_WRITE)?;
        expect_transfer_len(written, msg.len())?;

        let mut buf = [0u8; GET_INPUT_RESPONSE_LEN];
        let len = self.handle.read_bulk(ENDPOINT_IN, &mut buf, TIMEOUT_READ)?;
        expect_transfer_len(len, buf.len())?;

        Ok(buf[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
