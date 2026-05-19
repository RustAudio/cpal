use std::vec::IntoIter as VecIntoIter;

use super::Device;
pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

// TODO: Support enumerating earpiece vs headset vs speaker etc?
pub struct Devices(VecIntoIter<Device>);

impl Devices {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for Devices {
    fn default() -> Self {
        Self(vec![Device::new()].into_iter())
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

pub fn default_input_device() -> Option<Device> {
    Some(Device::default())
}

pub fn default_output_device() -> Option<Device> {
    Some(Device::default())
}
