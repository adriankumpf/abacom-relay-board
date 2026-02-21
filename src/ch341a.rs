use std::time::Duration;

use crate::DeviceHandle;
use crate::errors::Result;

const ENDPOINT_OUT: u8 = 0x02;
const ENDPOINT_IN: u8 = 0x82;
const TIMEOUT_WRITE: Duration = Duration::from_millis(100);
const TIMEOUT_READ: Duration = Duration::from_millis(10);

pub fn set_output(handle: &DeviceHandle, data: u8) -> Result {
    let msg = [0xA1, 0x6a, 0x1f, 0x00, 0x10, data, 0x3f, 0x00, 0x00, 0x00, 0x00];
    handle.write_bulk(ENDPOINT_OUT, &msg, TIMEOUT_WRITE)?;
    Ok(())
}

pub fn get_input(handle: &DeviceHandle) -> Result<Vec<u8>> {
    let msg = [0xA0];
    handle.write_bulk(ENDPOINT_OUT, &msg, TIMEOUT_WRITE)?;

    let mut buf = [0u8; 32];
    let len = handle.read_bulk(ENDPOINT_IN, &mut buf, TIMEOUT_READ)?;
    Ok(buf[..len].to_vec())
}
