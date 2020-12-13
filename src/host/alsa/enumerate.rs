use super::alsa;
use super::Device;
use {BackendSpecificError, DevicesError};

/// ALSA implementation for `Devices`.
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

                    return Some(Device {
                        name,
                        direction: hint.direction,
                        handles: Default::default(),
                    });
                }
            }
        }
    }
}

#[inline]
pub fn default_input_device() -> Option<Device> {
    Some(Default::default())
}

#[inline]
pub fn default_output_device() -> Option<Device> {
    Some(Default::default())
}

impl From<alsa::Error> for DevicesError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}
