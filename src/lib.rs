//! Library for controlling the ABACOM CH341A USB relay board.
//!
//! The board uses a CH341A USB-to-parallel chip to drive an Allegro A6275 shift register,
//! which in turn controls 8 relays. Communication is SPI-like: bits are clocked into the
//! A6275 serially (DATA + CLK), then latched to the outputs.
//!
//! # Relay addressing
//!
//! Relay state is an 8-bit bitmask: bit 0 = relay 1, bit 7 = relay 8, `1` = active.
//!
//! # Examples
//!
//! ```no_run
//! // Activate relays 1 and 3
//! arb::set_status(0b00000101, true, None).unwrap();
//!
//! // Read back current state
//! let status = arb::get_status(None).unwrap();
//!
//! // Turn everything off
//! arb::set_status(0, true, None).unwrap();
//! ```

use rusb::UsbContext;

mod ch341a;
mod errors;

pub use self::errors::{Error, Result};

/// USB vendor ID for the WCH CH341A chip.
const VENDOR_ID: u16 = 0x1a86;
/// USB product ID for the CH341A in parallel/GPIO mode.
const PRODUCT_ID: u16 = 0x5512;

// Allegro A6275 pin mapping on the CH341A D0–D7 GPIO lines.
const LATCH: u8 = 0x01; // D0 → A6275 Latch
const CLK: u8 = 0x08; // D3 → A6275 CLK
const DATA: u8 = 0x20; // D5 → A6275 Serial in
const READ: u8 = 0x80; // D7 ← A6275 Serial out

type DeviceHandle = rusb::DeviceHandle<rusb::Context>;
type Device = rusb::Device<rusb::Context>;

struct RelayBoard {
    handle: DeviceHandle,
}

impl RelayBoard {
    /// Finds and opens a relay board, optionally restricted to a given USB port.
    fn open(port: Option<u8>) -> Result<Self> {
        const EP_IFACE: u8 = 0;

        let device = find_device(port)?;
        let handle = device.open()?;

        if let Ok(true) = handle.kernel_driver_active(EP_IFACE) {
            handle.detach_kernel_driver(EP_IFACE)?;
        }

        handle.claim_interface(EP_IFACE)?;

        Ok(Self { handle })
    }

    /// Shifts 8 bits into the A6275 shift register (MSB first) without latching.
    fn shift_out_bits(&self, status: u8) -> Result {
        ch341a::set_output(&self.handle, 0)?;

        for i in 0..8 {
            if (status & (1 << (7 - i))) != 0 {
                ch341a::set_output(&self.handle, DATA)?;
                ch341a::set_output(&self.handle, CLK | DATA)?;
                ch341a::set_output(&self.handle, DATA)?;
            } else {
                ch341a::set_output(&self.handle, 0)?;
                ch341a::set_output(&self.handle, CLK)?;
                ch341a::set_output(&self.handle, 0)?;
            }
        }

        ch341a::set_output(&self.handle, 0)?;

        Ok(())
    }

    /// Shifts `status` into the A6275 and latches it to the relay outputs.
    ///
    /// If `verify` is true, reads back the shift register and returns
    /// [`Error::VerificationFailed`] if it doesn't match.
    fn set_status(&self, status: u8, verify: bool) -> Result {
        ch341a::set_output(&self.handle, 0)?;

        self.shift_out_bits(status)?;

        ch341a::set_output(&self.handle, LATCH)?;
        ch341a::set_output(&self.handle, 0)?;

        if verify && self.get_status()? != status {
            return Err(Error::VerificationFailed);
        }

        Ok(())
    }

    /// Reads the current A6275 shift register contents by clocking out 8 bits
    /// from the serial output (D7), then restores the register to the read value.
    fn get_status(&self) -> Result<u8> {
        let mut result = 0;

        ch341a::set_output(&self.handle, 0)?;

        for i in 0..8 {
            let input_state = ch341a::get_input(&self.handle)?;

            if (input_state & READ) != 0 {
                result |= 1 << (7 - i);
            }

            ch341a::set_output(&self.handle, CLK)?;
            ch341a::set_output(&self.handle, 0)?;
        }

        // Restore the shift register (clocking zeros in during read destroyed it).
        self.shift_out_bits(result)?;

        Ok(result)
    }

    fn reset(&self) -> Result {
        Ok(self.handle.reset()?)
    }
}

/// Returns whether `device` is the CH341A the relay board is built around.
fn is_relay_board(device: &Device) -> Result<bool> {
    let dd = device.device_descriptor()?;

    Ok(dd.vendor_id() == VENDOR_ID && dd.product_id() == PRODUCT_ID)
}

fn find_device(port: Option<u8>) -> Result<Device> {
    let context = rusb::Context::new()?;
    let mut found = None;

    for device in context.devices()?.iter() {
        if is_relay_board(&device)? && port.is_none_or(|p| device.port_number() == p) {
            if found.is_some() {
                return Err(Error::MultipleFound);
            }

            found = Some(device);
        }
    }

    found.ok_or(Error::NotFound)
}

/// Returns the current relay state as an 8-bit bitmask.
///
/// Bit 0 corresponds to relay 1, bit 7 to relay 8. A set bit means the relay is active.
///
/// Internally verifies the device is responsive by writing an inverted test pattern to the
/// shift register (without latching, so relay outputs are not disturbed) and reading it back.
/// Returns [`Error::BadDevice`] if the read-back doesn't match.
///
/// # Arguments
///
/// * `port` - USB port number to select a specific board when multiple are connected.
///
/// # Errors
///
/// * [`Error::NotFound`] — no relay board detected
/// * [`Error::MultipleFound`] — multiple boards detected and `port` is `None`
/// * [`Error::BadDevice`] — device did not respond correctly to the read-back test
pub fn get_status(port: Option<u8>) -> Result<u8> {
    let relay_board = RelayBoard::open(port)?;

    let old_status = relay_board.get_status()?;
    let test_status = !old_status;
    relay_board.shift_out_bits(test_status)?;
    let status = relay_board.get_status()?;

    if status != test_status {
        return Err(Error::BadDevice);
    }

    relay_board.shift_out_bits(old_status)?;

    Ok(old_status)
}

/// Activates the relays specified by `status`.
///
/// `status` is an 8-bit bitmask: bit 0 = relay 1, bit 7 = relay 8, `1` = active.
/// A value of `0` turns off all relays.
///
/// # Arguments
///
/// * `status` — bitmask of relays to activate.
/// * `verify` — if `true`, reads back the shift register after latching and returns
///   [`Error::VerificationFailed`] on mismatch.
/// * `port` — USB port number to select a specific board when multiple are connected.
///
/// # Example
///
/// ```no_run
/// // Activate relays 1, 2, 4, 5 and 6
/// arb::set_status(0b00110111, true, None).unwrap();
/// ```
pub fn set_status(status: u8, verify: bool, port: Option<u8>) -> Result {
    RelayBoard::open(port)?.set_status(status, verify)
}

/// Performs a USB reset on the relay board.
///
/// # Arguments
///
/// * `port` — USB port number to select a specific board when multiple are connected.
pub fn reset(port: Option<u8>) -> Result {
    RelayBoard::open(port)?.reset()
}
