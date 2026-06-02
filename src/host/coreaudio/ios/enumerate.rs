use std::vec::IntoIter as VecIntoIter;

use super::Device;
pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

// TODO: Support enumerating earpiece vs headset vs speaker etc?
pub struct Devices(pub(super) VecIntoIter<Device>);

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

pub fn default_input_device() -> Option<Device> {
    Some(Device)
}

pub fn default_output_device() -> Option<Device> {
    Some(Device)
}
