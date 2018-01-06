extern crate libusb;

mod errors;
mod ch341a;

use errors::{Error, Result};

const VENDOR_ID: u16 = 0x1a86;
const PRODUCT_ID: u16 = 0x5512;

// Allegro A6275 driver chip
const LATCH: u8 = 0x01; // to A6275 Latch in
const CLK: u8 = 0x08; // to A6275 CLK in
const DATA: u8 = 0x20; // to A6275 Serial in
const READ: u8 = 0x80; // from A6275 Serial out

struct RelayBoard<'a> {
    device: libusb::Device<'a>,
}

impl<'a> RelayBoard<'a> {
    fn from(device: libusb::Device<'a>) -> Option<Self> {
        device
            .device_descriptor()
            .and_then(|device_desc| {
                if device_desc.vendor_id() != VENDOR_ID || device_desc.product_id() != PRODUCT_ID {
                    return Err(libusb::Error::Other);
                };

                Ok(Self {
                    device: device.clone(),
                })
            })
            .ok()
    }

    fn get_port(&self) -> u8 {
        self.device.port_number()
    }

    fn open_device(&self) -> Result<libusb::DeviceHandle> {
        const EP_IFACE: u8 = 0;

        let mut handle = self.device.open()?;

        if let Ok(true) = handle.kernel_driver_active(EP_IFACE) {
            handle.detach_kernel_driver(EP_IFACE)?;
        };

        handle.claim_interface(EP_IFACE)?;

        Ok(handle)
    }

    fn set_active_relays(&self, relays: u8) -> Result {
        let mut handle = self.open_device()?;

        ch341a::set_output(&mut handle, 0)?; //# Latch low

        for i in 0..8 {
            if (relays & (1 << (7 - i))) != 0 {
                ch341a::set_output(&mut handle, DATA)?; // DATA high
                ch341a::set_output(&mut handle, CLK)?; // CLK high
                ch341a::set_output(&mut handle, DATA)?; // CLK low
            } else {
                ch341a::set_output(&mut handle, 0)?; // DATA low
                ch341a::set_output(&mut handle, CLK)?; // CLK high
                ch341a::set_output(&mut handle, 0)?; // CLK low
                ch341a::set_output(&mut handle, 0)?; // All lines low
            }
        }
        ch341a::set_output(&mut handle, LATCH)?; // Latch high
        ch341a::set_output(&mut handle, 0)?; // Latch, CLK, OE low

        if self.get_active_relays(handle)? != relays {
            return Err(Error::VerificationFailed);
        }

        Ok(())
    }

    fn get_active_relays(&self, mut handle: libusb::DeviceHandle) -> Result<u8> {
        let mut result = 0;

        ch341a::set_output(&mut handle, 0)?; // all lines low

        // shift out bit 0..7 from A6275...
        for i in 0..8 {
            let input_state = ch341a::get_input(&mut handle)?[0]; //Get status of CH341A D0..D7 lines

            // READ bits from A6275 Serial out (at D7 line).
            if (input_state & READ) != 0 {
                result = result | (1 << (7 - i));
            }

            // generate CLK pulse for next bit from A6275
            ch341a::set_output(&mut handle, CLK)?; // CLK high
            ch341a::set_output(&mut handle, 0)?; // CLK low
        }

        Ok(result)
    }
}

pub fn switch_relays(relays: u8, port: Option<u8>) -> Result {
    let context = libusb::Context::new()?;

    let relay_boards: Vec<_> = context
        .devices()?
        .iter()
        .filter_map(RelayBoard::from)
        .collect();

    let relay_board = match relay_boards.len() {
        0 => Err(Error::NotFound),
        1 => Ok(&relay_boards[0]),
        _ => match port {
            None => Err(Error::MultipleFound),
            Some(p) => match relay_boards.iter().find(|rb| rb.get_port() == p) {
                None => Err(Error::NotFound),
                Some(rb) => Ok(rb),
            },
        },
    };

    relay_board?.set_active_relays(relays)
}
