extern crate libusb;

mod errors;
mod ch341a;

pub use errors::{Error, Result};

const VENDOR_ID: u16 = 0x1a86;
const PRODUCT_ID: u16 = 0x5512;

// Allegro A6275 driver chip
const LATCH: u8 = 0x01; // to A6275 Latch in
const CLK: u8 = 0x08; // to A6275 CLK in
const DATA: u8 = 0x20; // to A6275 Serial in
const READ: u8 = 0x80; // from A6275 Serial out

#[derive(Clone)]
struct RelayBoard<'a> {
    device: libusb::Device<'a>,
}

impl<'a> RelayBoard<'a> {
    fn from(device: libusb::Device<'a>) -> Result<Option<RelayBoard<'a>>> {
        let dd = device.device_descriptor()?;

        if dd.vendor_id() != VENDOR_ID || dd.product_id() != PRODUCT_ID {
            return Ok(None);
        };

        Ok(Some(RelayBoard { device: device }))
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

    fn shift_out_bits(&self, handle: &libusb::DeviceHandle, relays: u8) -> Result {
        ch341a::set_output(handle, 0)?; // All lines low

        for i in 0..8 {
            if (relays & (1 << (7 - i))) != 0 {
                // relay on
                ch341a::set_output(handle, DATA)?; // DATA high
                ch341a::set_output(handle, (CLK | DATA))?; // CLK high
                ch341a::set_output(handle, DATA)?; // CLK low
            } else {
                // relay off
                ch341a::set_output(handle, 0)?; // DATA low
                ch341a::set_output(handle, CLK)?; // CLK high
                ch341a::set_output(handle, 0)?; // CLK low
                                                // ch341a::set_output(&mut handle, 0)?; // All lines low
            }
        }

        ch341a::set_output(handle, 0)?; // All lines 0

        Ok(())
    }

    fn set_active_relays(&self, handle: &libusb::DeviceHandle, relays: u8, verify: bool) -> Result {
        ch341a::set_output(handle, 0)?; // Latch low

        self.shift_out_bits(handle, relays)?;

        ch341a::set_output(handle, LATCH)?; // Latch high
        ch341a::set_output(handle, 0)?; // Latch, CLK, OE low

        if verify && self.get_active_relays(handle)? != relays {
            return Err(Error::VerificationFailed);
        }

        Ok(())
    }

    fn get_active_relays(&self, handle: &libusb::DeviceHandle) -> Result<u8> {
        let mut result = 0;

        ch341a::set_output(handle, 0)?; // all lines low

        // shift out bit 0..7 from A6275...
        for i in 0..8 {
            let input_state = ch341a::get_input(handle)?[0]; //Get status of CH341A D0..D7 lines

            // READ bits from A6275 Serial out (at D7 line).
            if (input_state & READ) != 0 {
                result |= 1 << (7 - i);
            }

            // generate CLK pulse for next bit from A6275
            ch341a::set_output(handle, CLK)?; // CLK high
            ch341a::set_output(handle, 0)?; // CLK low
        }

        self.shift_out_bits(handle, result)?;

        Ok(result)
    }
}

fn find_relay_board(context: &libusb::Context, port: Option<u8>) -> Result<RelayBoard> {
    let mut relay_boards = vec![];

    for device in context.devices()?.iter() {
        if let Some(rb) = RelayBoard::from(device)? {
            relay_boards.push(rb);
        }
    }

    match relay_boards.len() {
        0 => Err(Error::NotFound),
        1 => Ok(relay_boards[0].clone()),
        _ => match port {
            None => Err(Error::MultipleFound),
            Some(p) => match relay_boards.iter().find(|rb| rb.get_port() == p) {
                None => Err(Error::NotFound),
                Some(rb) => Ok(rb.clone()),
            },
        },
    }
}

pub fn get_relays(port: Option<u8>) -> Result<u8> {
    let context = libusb::Context::new()?;
    let relay_board = find_relay_board(&context, port)?;
    let handle = relay_board.open_device()?;

    let old_status = relay_board.get_active_relays(&handle)?;
    let test_status = !old_status;
    relay_board.shift_out_bits(&handle, test_status)?;
    let status = relay_board.get_active_relays(&handle)?;

    if status != test_status {
        return Err(Error::BadDevice);
    }

    relay_board.shift_out_bits(&handle, old_status)?;

    Ok(old_status)
}

pub fn switch_relays(relays: u8, verify: bool, port: Option<u8>) -> Result {
    let context = libusb::Context::new()?;
    let relay_board = find_relay_board(&context, port)?;
    let handle = relay_board.open_device()?;

    relay_board.set_active_relays(&handle, relays, verify)
}
