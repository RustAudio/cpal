use device::{init_devices, ClassType, Device, Devices};

use crate::traits::HostTrait;
mod device;
mod stream;
#[derive(Debug)]
pub struct Host(Vec<Device>);

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        let devices = init_devices().ok_or(crate::HostUnavailable)?;
        Ok(Host(devices))
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;
    fn is_available() -> bool {
        true
    }
    fn devices(&self) -> Result<Self::Devices, crate::DevicesError> {
        Ok(self.0.clone().into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.0
            .iter()
            .find(|device| matches!(device.class_type(), ClassType::DefaultSink))
            .cloned()
    }
    fn default_output_device(&self) -> Option<Self::Device> {
        self.0
            .iter()
            .find(|device| matches!(device.class_type(), ClassType::DefaultOutput))
            .cloned()
    }
}
