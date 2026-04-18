use crate::traits::HostTrait;
use device::{init_devices, Class, Device, Devices};
use stream::PwInitGuard;

mod device;
mod stream;
mod utils;

pub struct Host {
    // Keeps PipeWire initialized for the lifetime of the host, preventing
    // pw_deinit() from running between device enumeration and stream creation.
    _pw: PwInitGuard,
    devices: Vec<Device>,
}

impl Host {
    pub fn new() -> Result<Self, crate::Error> {
        let _pw = PwInitGuard::new();
        let devices = init_devices().ok_or_else(|| {
            crate::Error::with_message(
                crate::ErrorKind::HostUnavailable,
                "PipeWire host initialization failed",
            )
        })?;
        Ok(Host { _pw, devices })
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;
    fn is_available() -> bool {
        utils::find_socket_path().is_some()
    }
    fn devices(&self) -> Result<Self::Devices, crate::Error> {
        Ok(self.devices.clone().into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.devices
            .iter()
            .find(|device| matches!(device.class(), Class::DefaultInput))
            .cloned()
    }
    fn default_output_device(&self) -> Option<Self::Device> {
        self.devices
            .iter()
            .find(|device| matches!(device.class(), Class::DefaultOutput))
            .cloned()
    }
}
