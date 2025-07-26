use super::alsa;
use super::{Device, DeviceHandles};
use crate::{BackendSpecificError, DevicesError};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// ALSA's implementation for `Devices`.
pub struct Devices {
    hint_iter: Option<alsa::device_name::HintIter>,
    enumerated_pcm_ids: HashSet<String>,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices {
            hint_iter: None,
            enumerated_pcm_ids: HashSet::new(),
        })
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

fn try_open_device(pcm_id: &str, name: String) -> Option<Device> {
    // Try to open handles during enumeration
    let handles = DeviceHandles::open(pcm_id).unwrap_or_else(|_| {
        // If opening fails during enumeration, create default handles
        // The actual opening will be attempted when the device is used
        DeviceHandles::default()
    });

    // Include all devices from ALSA hints (matches `aplay -L` behavior)
    // Even devices that can't be opened during enumeration are valid for selection
    Some(Device {
        name,
        pcm_id: pcm_id.to_string(),
        handles: Arc::new(Mutex::new(handles)),
    })
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        // Enumerate ALL devices from ALSA hints (same as aplay -L)
        if self.hint_iter.is_none() {
            match alsa::device_name::HintIter::new_str(None, "pcm") {
                Ok(iter) => self.hint_iter = Some(iter),
                Err(_) => return None, // If hints fail, we're done
            }
        }

        if let Some(ref mut hint_iter) = self.hint_iter {
            loop {
                match hint_iter.next() {
                    Some(hint) => {
                        let name = match hint.name {
                            None => continue,
                            Some(name) => name,
                        };

                        // Skip if we've already enumerated this device by PCM ID
                        if self.enumerated_pcm_ids.contains(&name) {
                            continue;
                        }

                        // Include all devices from hints (same as aplay -L)
                        if let Some(device) = try_open_device(&name, name.clone()) {
                            self.enumerated_pcm_ids.insert(name.clone());
                            return Some(device);
                        }
                    }
                    None => return None, // All devices enumerated
                }
            }
        }

        None
    }
}

#[inline]
pub fn default_input_device() -> Option<Device> {
    Some(Device {
        name: "default".to_owned(),
        pcm_id: "default".to_owned(),
        handles: Arc::new(Mutex::new(Default::default())),
    })
}

#[inline]
pub fn default_output_device() -> Option<Device> {
    Some(Device {
        name: "default".to_owned(),
        pcm_id: "default".to_owned(),
        handles: Arc::new(Mutex::new(Default::default())),
    })
}

impl From<alsa::Error> for DevicesError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}
