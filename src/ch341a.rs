//! Low-level CH341A USB bulk transfer interface.
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

use crate::DeviceHandle;
use crate::errors::{Error, Result};

const ENDPOINT_OUT: u8 = 0x02;
const ENDPOINT_IN: u8 = 0x82;
const TIMEOUT_WRITE: Duration = Duration::from_millis(100);
const TIMEOUT_READ: Duration = Duration::from_millis(10);
const SET_OUTPUT_MSG_LEN: usize = 11;
const GET_INPUT_RESPONSE_LEN: usize = 6;

fn expect_transfer_len(actual: usize, expected: usize) -> Result {
    if actual == expected {
        return Ok(());
    }

    Err(Error::UnexpectedTransferLength { expected, actual })
}

/// Sets the CH341A D0–D7 output lines to `data`.
///
/// Each bit in `data` corresponds to one GPIO line. On the ABACOM relay board:
/// - Bit 0 (0x01): A6275 LATCH
/// - Bit 3 (0x08): A6275 CLK
/// - Bit 5 (0x20): A6275 Serial DATA in
pub fn set_output(handle: &DeviceHandle, data: u8) -> Result {
    let msg = [
        0xA1, 0x6a, 0x1f, 0x00, 0x10, data, 0x3f, 0x00, 0x00, 0x00, 0x00,
    ];
    let written = handle.write_bulk(ENDPOINT_OUT, &msg, TIMEOUT_WRITE)?;
    expect_transfer_len(written, SET_OUTPUT_MSG_LEN)?;
    Ok(())
}

/// Reads the CH341A D0–D7 input lines and returns byte 0 (D7–D0).
///
/// On the ABACOM relay board, bit 7 (D7) carries the A6275 serial output,
/// used to read back the current shift register contents.
pub fn get_input(handle: &DeviceHandle) -> Result<u8> {
    let msg = [0xA0];
    let written = handle.write_bulk(ENDPOINT_OUT, &msg, TIMEOUT_WRITE)?;
    expect_transfer_len(written, msg.len())?;

    let mut buf = [0u8; GET_INPUT_RESPONSE_LEN];
    let len = handle.read_bulk(ENDPOINT_IN, &mut buf, TIMEOUT_READ)?;
    expect_transfer_len(len, GET_INPUT_RESPONSE_LEN)?;
    Ok(buf[0])
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
