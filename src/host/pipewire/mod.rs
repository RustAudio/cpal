use device::{init_devices, Device};
mod device;

#[derive(Debug)]
pub struct Host(Vec<Device>);

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        let devices = init_devices().ok_or(crate::HostUnavailable)?;
        Ok(Host(devices))
    }
}
