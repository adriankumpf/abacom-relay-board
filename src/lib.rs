extern crate libusb;

mod traits;
mod errors;

use traits::{Device, DeviceDescriptor};

pub use errors::Error;
pub use errors::Result;

const VENDOR_ID: u16 = 0x2109; // 0x1a86;
const PRODUCT_ID: u16 = 0x0100; // 0x5512;

type RelayBoards = Vec<RelayBoard>;

#[derive(Debug, PartialEq)]
pub struct RelayBoard {
    pub port: u8,
    pub bus: u8,
    pub addr: u8,
}

impl RelayBoard {
    fn new(port: u8, bus: u8, addr: u8) -> Self {
        Self { port, bus, addr }
    }
    fn from<C, D>(device: D) -> Option<Self>
    where
        C: DeviceDescriptor,
        D: Device<C>,
    {
        device
            .device_descriptor()
            .and_then(|device_desc| {
                if device_desc.vendor_id() != VENDOR_ID || device_desc.product_id() != PRODUCT_ID {
                    return Err(libusb::Error::Other);
                };

                Ok(Self::new(
                    device.port_number(),
                    device.bus_number(),
                    device.address(),
                ))
            })
            .ok()
    }
    pub fn activate<'a>(&self, _relays: Vec<u8>) -> Result<()> {
        Ok(())
    }
}

fn get_relay_boards() -> Result<RelayBoards> {
    let context = libusb::Context::new()?;
    let devices = context.devices()?;
    let boards = devices.iter().filter_map(RelayBoard::from).collect();
    Ok(boards)
}

fn do_switch_relays(relay_boards: RelayBoards, relays: Vec<u8>, port: Option<u8>) -> Result<()> {
    match relay_boards.len() {
        0 => Err(Error::NotFound),
        1 => relay_boards[0].activate(relays),
        _ => match port {
            None => Err(Error::MultipleFound),
            Some(p) => match relay_boards.iter().find(|rb| rb.port == p) {
                None => Err(Error::NotFound),
                Some(relay_board) => relay_board.activate(relays),
            },
        },
    }
}

pub fn switch_relays(relays: Vec<u8>, port: Option<u8>) -> Result<()> {
    do_switch_relays(get_relay_boards()?, relays, port)
}

#[cfg(test)]
mod tests {
    use super::*;

    use traits::tests::TestDevice;

    #[test]
    fn relay_board_from_device_success() {
        let device = TestDevice {
            vendor_id: 0x1a86,
            product_id: 0x5512,
            addr: 10,
            bus: 20,
            port: 30,
        };

        assert_eq!(
            RelayBoard::from(device),
            Some(RelayBoard {
                addr: 10,
                bus: 20,
                port: 30,
            })
        )
    }

    #[test]
    fn relay_board_from_device_none() {
        let device = TestDevice {
            vendor_id: 0x0000,
            product_id: 0x5512,
            addr: 0,
            bus: 0,
            port: 0,
        };

        assert_eq!(RelayBoard::from(device), None);

        let device = TestDevice {
            vendor_id: 0x1a86,
            product_id: 0x0000,
            addr: 0,
            bus: 0,
            port: 0,
        };

        assert_eq!(RelayBoard::from(device), None);
    }

    #[test]
    fn switch_relays_not_found() {
        assert_eq!(
            do_switch_relays(vec![], vec![1], None),
            Err(Error::NotFound)
        )
    }

    #[test]
    fn switch_relays_multiple_found() {
        assert_eq!(
            do_switch_relays(
                vec![RelayBoard::new(10, 4, 4), RelayBoard::new(20, 4, 4)],
                vec![1],
                None
            ),
            Err(Error::MultipleFound)
        )
    }

    #[test]
    fn switch_relays_wrong_port() {
        assert_eq!(
            do_switch_relays(
                vec![RelayBoard::new(10, 4, 4), RelayBoard::new(20, 4, 4)],
                vec![1],
                Some(11)
            ),
            Err(Error::NotFound)
        )
    }
}
