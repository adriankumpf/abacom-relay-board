use std::slice;
use std::time::Duration;

use crate::errors::{Error, Result};
use crate::DeviceHandle;

const ENDPOINT_IN: u8 = 0x02;
const ENDPOINT_OUT: u8 = 0x82;
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

fn write(handle: &DeviceHandle, mut data: Vec<u8>) -> Result {
    let buf = unsafe { slice::from_raw_parts_mut(data[..].as_mut_ptr(), data.capacity()) };

    match handle.write_bulk(ENDPOINT_IN, buf, Duration::from_millis(100)) {
        Err(err) => Err(Error::Usb(err)),
        Ok(_len) => Ok(()),
    }
}

fn read(handle: &DeviceHandle) -> Result<Vec<u8>> {
    let mut vec = Vec::<u8>::with_capacity(READ_BUF_SIZE);
    let buf = unsafe { slice::from_raw_parts_mut(vec[..].as_mut_ptr(), vec.capacity()) };

    match handle.read_bulk(ENDPOINT_OUT, buf, Duration::from_millis(10)) {
        Err(err) => Err(Error::Usb(err)),
        Ok(len) => {
            if len > READ_BUF_SIZE {
                return Err(Error::UnsafeRead);
            }

            unsafe { vec.set_len(len) };

            Ok(Vec::from(buf))
        }
    }
}
