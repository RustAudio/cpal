use screencapturekit::sc_shareable_content::SCShareableContent;

use std::vec::IntoIter as VecIntoIter;

use crate::{BackendSpecificError, DevicesError, SupportedStreamConfigRange};

use super::Device;

pub struct Devices(VecIntoIter<Device>);

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        let sc_shareable_content = SCShareableContent::try_current()
            .map_err(|description| BackendSpecificError { description })?;

        let mut res = Vec::new();
        for display in sc_shareable_content.displays.into_iter() {
            res.push(Device::new(display));
        }

        Ok(Devices(res.into_iter()))
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

pub fn default_input_device() -> Option<Device> {
    let devices = Devices::new().ok()?;
    devices.into_iter().next()
}

pub fn default_output_device() -> Option<Device> {
    None
}

pub type SupportedInputConfigs = VecIntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = VecIntoIter<SupportedStreamConfigRange>;
