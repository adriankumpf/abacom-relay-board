extern crate libusb;

mod helper;

const USB_RELAYBOARD_VENDOR_ID: u16 = 0x1a86;
const USB_RELAYBOARD_PRODUCT_ID: u16 = 0x5512;

#[derive(Debug, PartialEq)]
pub struct RelayBoard {
    port: u8,
    bus: u8,
    addr: u8,
}

impl RelayBoard {
    fn from<C, D>(device: D) -> Option<Self>
    where
        C: helper::DeviceDescriptor,
        D: helper::Device<C>,
    {
        device
            .device_descriptor()
            .and_then(|device_desc| {
                if device_desc.vendor_id() != USB_RELAYBOARD_VENDOR_ID
                    || device_desc.product_id() != USB_RELAYBOARD_PRODUCT_ID
                {
                    return Err(libusb::Error::NoDevice);
                };

                Ok(Self {
                    port: device.port_number(),
                    bus: device.bus_number(),
                    addr: device.address(),
                })
            })
            .ok()
    }
}

pub fn list_relay_boards() -> Result<Vec<RelayBoard>, libusb::Error> {
    let context = libusb::Context::new()?;
    let devices = context.devices()?;

    let relay_boards: Vec<RelayBoard> = devices.iter().filter_map(RelayBoard::from).collect();

    Ok(relay_boards)
}

#[cfg(test)]
mod tests {
    use libusb;
    use super::*;
    use helper::tests::TestDevice;

    #[test]
    fn relay_board_from_device_success() {
        let device = TestDevice {
            vendor_id: 0x1a86,
            product_id: 0x5512,
            addr: 110,
            bus: 220,
            port: 330,
        };

        assert_eq!(
            RelayBoard::from(device),
            Some(RelayBoard {
                addr: 110,
                bus: 220,
                port: 330,
            })
        )
    }

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
}
