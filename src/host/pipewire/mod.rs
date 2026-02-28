use crate::traits::HostTrait;
use device::{init_devices, Class, Device, Devices};
mod device;
mod stream;
mod utils;

#[inline]
fn pipewire_available() -> bool {
    let dir = std::env::var("PIPEWIRE_RUNTIME_DIR")
        .or_else(|_| std::env::var("XDG_RUNTIME_DIR"))
        .unwrap_or_default();
    std::path::Path::new(&dir).join("pipewire-0").exists()
}

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
        pipewire_available()
    }
    fn devices(&self) -> Result<Self::Devices, crate::DevicesError> {
        Ok(self.0.clone().into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.0
            .iter()
            .find(|device| matches!(device.class(), Class::DefaultInput))
            .cloned()
    }
    fn default_output_device(&self) -> Option<Self::Device> {
        self.0
            .iter()
            .find(|device| matches!(device.class(), Class::DefaultOutput))
            .cloned()
    }
}
