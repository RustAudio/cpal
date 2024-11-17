use std::vec::IntoIter as VecIntoIter;

use cidre::sc;

use crate::{BackendSpecificError, DevicesError, SupportedStreamConfigRange};

use super::Device;

pub struct Devices(VecIntoIter<Device>);

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        let (tx, rx) = std::sync::mpsc::channel();
        sc::ShareableContent::current_with_ch(move |sc, e| {
            let res = if let Some(err) = e {
                Result::Err(BackendSpecificError {
                    description: format!("{err}"),
                })
            } else if let Some(sc) = sc {
                Result::Ok(sc.retained())
            } else {
                Result::Err(BackendSpecificError {
                    description: "Failed to get current shareable content".to_string(),
                })
            };
            tx.send(res).unwrap();
        });
        let sc_shareable_content = rx.recv().unwrap()?;

        let mut res = Vec::new();
        for display in sc_shareable_content.displays().iter() {
            res.push(Device::new(display.retained()));
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
