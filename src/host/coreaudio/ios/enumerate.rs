use std::vec::IntoIter as VecIntoIter;

use crate::DevicesError;
use crate::SupportedStreamConfigRange;

use super::Device;

pub type SupportedInputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;

// TODO: Support enumerating earpiece vs headset vs speaker etc?
pub struct Devices(VecIntoIter<Device>);

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Self::default())
    }
}

impl Default for Devices {
    fn default() -> Devices {
        Devices(vec![Device].into_iter())
    }
}

impl Iterator for Devices {
    type Item = Device;

    #[inline]
    fn next(&mut self) -> Option<Device> {
        self.0.next()
    }
}

#[inline]
pub fn default_input_device() -> Option<Device> {
    Some(Device)
}

#[inline]
pub fn default_output_device() -> Option<Device> {
    Some(Device)
}
