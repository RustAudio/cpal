use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use super::{
    alsa, {Device, DeviceHandles},
};
use crate::{BackendSpecificError, DevicesError};

/// ALSA's implementation for `Devices`.
pub struct Devices {
    hint_iter: alsa::device_name::HintIter,
    enumerated_pcm_ids: HashSet<String>,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        // Enumerate ALL devices from ALSA hints (same as aplay -L)
        alsa::device_name::HintIter::new_str(None, "pcm")
            .map(|hint_iter| Self {
                hint_iter,
                enumerated_pcm_ids: HashSet::new(),
            })
            .map_err(DevicesError::from)
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

fn open_device(pcm_id: &str, desc: Option<String>) -> Device {
    // Try to open handles during enumeration
    let handles = DeviceHandles::open(pcm_id).unwrap_or_else(|_| {
        // If opening fails during enumeration, create default handles
        // The actual opening will be attempted when the device is used
        DeviceHandles::default()
    });

    // Include all devices from ALSA hints (matches `aplay -L` behavior)
    // Even devices that can't be opened during enumeration are valid for selection
    Device {
        pcm_id: pcm_id.to_owned(),
        desc,
        handles: Arc::new(Mutex::new(handles)),
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        loop {
            let hint = self.hint_iter.next()?;
            let (pcm_id, desc) = match (hint.name, hint.desc) {
                (Some(name), desc) => (name, desc),
                _ => continue, // Skip hints without a valid PCM ID
            };

            let device = open_device(&pcm_id, desc);
            self.enumerated_pcm_ids.insert(pcm_id);
            return Some(device);
        }
    }
}

#[inline]
pub fn default_input_device() -> Option<Device> {
    Some(default_device())
}

#[inline]
pub fn default_output_device() -> Option<Device> {
    Some(default_device())
}

#[inline]
pub fn default_device() -> Device {
    Device {
        pcm_id: "default".to_string(),
        desc: Some("Default Audio Device".to_string()),
        handles: Arc::new(Mutex::new(Default::default())),
    }
}

impl From<alsa::Error> for DevicesError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}
