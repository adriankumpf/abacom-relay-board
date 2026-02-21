use std::time::Duration;

use crate::DeviceHandle;
use crate::errors::{Error, Result};

const ENDPOINT_OUT: u8 = 0x02;
const ENDPOINT_IN: u8 = 0x82;
const READ_BUF_SIZE: usize = 32;

pub fn set_output(handle: &DeviceHandle, data: u8) -> Result {
    let msg = vec![
        0xA1, 0x6a, 0x1f, 0x00, 0x10, data, 0x3f, 0x00, 0x00, 0x00, 0x00,
    ];
    write(handle, msg)
}

pub fn get_input(handle: &DeviceHandle) -> Result<Vec<u8>> {
    let msg = vec![0xA0];
    write(handle, msg)?;
    read(handle)
}

fn write(handle: &DeviceHandle, data: Vec<u8>) -> Result {
    match handle.write_bulk(ENDPOINT_OUT, &data, Duration::from_millis(100)) {
        Err(err) => Err(Error::Usb(err)),
        Ok(_len) => Ok(()),
    }
}

fn read(handle: &DeviceHandle) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; READ_BUF_SIZE];

    match handle.read_bulk(ENDPOINT_IN, &mut buf, Duration::from_millis(10)) {
        Err(err) => Err(Error::Usb(err)),
        Ok(len) => {
            buf.truncate(len);
            Ok(buf)
        }
    }
}
