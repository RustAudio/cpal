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

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        loop {
            let hint = self.hint_iter.next()?;
            if let Ok(device) = Device::try_from(hint) {
                if self.enumerated_pcm_ids.insert(device.pcm_id.clone()) {
                    return Some(device);
                } else {
                    // Skip duplicate PCM IDs
                    continue;
                }
            }
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

impl TryFrom<alsa::device_name::Hint> for Device {
    type Error = BackendSpecificError;

    fn try_from(hint: alsa::device_name::Hint) -> Result<Self, Self::Error> {
        let pcm_id = hint.name.ok_or_else(|| BackendSpecificError {
            description: "ALSA hint missing PCM ID".to_string(),
        })?;

        // Don't try to open handles during enumeration to avoid ALSA logging errors
        // for device templates (e.g., with $CARD placeholders) or unavailable devices.
        // Opening will be attempted when the device is actually used.
        let handles = DeviceHandles::default();

        // Include all devices from ALSA hints (matches `aplay -L` behavior)
        Ok(Self {
            pcm_id: pcm_id.to_owned(),
            desc: hint.desc,
            handles: Arc::new(Mutex::new(handles)),
        })
    }
}
