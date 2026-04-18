use crate::traits::HostTrait;
use device::{init_devices, Class, Device, Devices};
use std::path::Path;
use stream::PwInitGuard;

mod device;
mod stream;
mod utils;

#[inline]
fn pipewire_available() -> bool {
    fn has_socket(dir: &Path) -> bool {
        dir.join("pipewire-0").exists()
    }

    if let Ok(dir) = std::env::var("PIPEWIRE_RUNTIME_DIR") {
        return has_socket(Path::new(&dir));
    }

    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        let path = Path::new(&xdg);
        // Snap sets XDG_RUNTIME_DIR to a snap-specific subdirectory but keeps
        // the PipeWire socket in the parent.
        return has_socket(path) || path.parent().is_some_and(has_socket);
    }

    false
}

pub struct Host {
    // Keeps PipeWire initialized for the lifetime of the host, preventing
    // pw_deinit() from running between device enumeration and stream creation.
    _pw: PwInitGuard,
    devices: Vec<Device>,
}

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        let _pw = PwInitGuard::new();
        let devices = init_devices().ok_or(crate::HostUnavailable)?;
        Ok(Host { _pw, devices })
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;
    fn is_available() -> bool {
        pipewire_available()
    }
    fn devices(&self) -> Result<Self::Devices, crate::DevicesError> {
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
