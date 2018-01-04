use libusb;

pub trait Device<T: DeviceDescriptor> {
    fn address(&self) -> u8;
    fn bus_number(&self) -> u8;
    fn device_descriptor(&self) -> libusb::Result<T>;
    fn port_number(&self) -> u8;
}

pub trait DeviceDescriptor {
    fn vendor_id(&self) -> u16;
    fn product_id(&self) -> u16;
}

impl DeviceDescriptor for libusb::DeviceDescriptor {
    fn vendor_id(&self) -> u16 {
        self.vendor_id()
    }
    fn product_id(&self) -> u16 {
        self.product_id()
    }
}

impl<'a> Device<libusb::DeviceDescriptor> for libusb::Device<'a> {
    fn device_descriptor(&self) -> libusb::Result<libusb::DeviceDescriptor> {
        self.device_descriptor()
    }
    fn address(&self) -> u8 {
        self.address()
    }
    fn bus_number(&self) -> u8 {
        self.bus_number()
    }
    fn port_number(&self) -> u8 {
        self.port_number()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use libusb;

    #[derive(Debug)]
    pub struct TestDevice {
        pub product_id: u16,
        pub vendor_id: u16,
        pub addr: u8,
        pub bus: u8,
        pub port: u8,
    }

    pub struct TestDeviceDescriptor {
        product_id: u16,
        vendor_id: u16,
    }

    impl DeviceDescriptor for TestDeviceDescriptor {
        fn vendor_id(&self) -> u16 {
            self.vendor_id
        }
        fn product_id(&self) -> u16 {
            self.product_id
        }
    }

    impl Device<TestDeviceDescriptor> for TestDevice {
        fn device_descriptor(&self) -> libusb::Result<TestDeviceDescriptor> {
            Ok(TestDeviceDescriptor {
                vendor_id: self.vendor_id,
                product_id: self.product_id,
            })
        }
        fn address(&self) -> u8 {
            self.addr
        }
        fn bus_number(&self) -> u8 {
            self.bus
        }
        fn port_number(&self) -> u8 {
            self.port
        }
    }
}
