use super::alsa;
use super::{Device, DeviceHandles};
use crate::{BackendSpecificError, DevicesError};
use std::sync::{Arc, Mutex};

/// ALSA's implementation for `Devices`.
pub struct Devices {
    hint_iter: alsa::device_name::HintIter,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices {
            hint_iter: alsa::device_name::HintIter::new_str(None, "pcm")?,
        })
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        loop {
            match self.hint_iter.next() {
                None => return None,
                Some(hint) => {
                    let name = match hint.name {
                        None => continue,
                        // Ignoring the `null` device.
                        Some(name) if name == "null" => continue,
                        Some(name) => name,
                    };

                    if let Ok(handles) = DeviceHandles::open(&name) {
                        return Some(Device {
                            name,
                            handles: Arc::new(Mutex::new(handles)),
                        });
                    }
                }
            }
        }
    }
}

#[inline]
pub fn default_input_device() -> Option<Device> {
    Some(Device {
        name: "default".to_owned(),
        handles: Arc::new(Mutex::new(Default::default())),
    })
}

#[inline]
pub fn default_output_device() -> Option<Device> {
    Some(Device {
        name: "default".to_owned(),
        handles: Arc::new(Mutex::new(Default::default())),
    })
}

impl From<alsa::Error> for DevicesError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}
