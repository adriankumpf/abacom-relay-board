extern crate libusb;

const USB_RELAYBOARD_VENDOR_ID: u16 = 0x1a86;
const USB_RELAYBOARD_PRODUCT_ID: u16 = 0x5512;

#[derive(Debug)]
pub struct RelayBoard {
    port: u8,
    bus: u8,
    addr: u8,
}

impl RelayBoard {
    fn from(device: libusb::Device) -> Option<Self> {
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
