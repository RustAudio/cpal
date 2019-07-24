use crate::{BackendSpecificError, DevicesError, host::alsa::Device};
use alsa::{PCM, Direction, device_name::HintIter};

/// ALSA implementation for `Devices`.
pub struct Devices {
    inner: HintIter
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices {
            // TODO use CStr when constructor is constant
            inner: HintIter::new_str(None, "pcm").map_err(|e| DevicesError::from(BackendSpecificError {
                description: e.to_string()
            }))?
        })
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        loop {
            let next = self.inner.next()?;
            // skip null device
            let name = match next.name {
                Some(ref name) if name == "null" => continue,
                Some(name) => name,
                None => continue,
            };
            // check device has output or input
            let has_available_output = PCM::new(&name, Direction::Playback, true).is_ok();
            let has_available_input = PCM::new(&name, Direction::Capture, true).is_ok();
            if has_available_output || has_available_input {
                return Some(Device(name));
            }
        }
    }
}

#[inline]
pub fn default_input_device() -> Option<Device> {
    Some(Device("default".to_owned()))
}

#[inline]
pub fn default_output_device() -> Option<Device> {
    Some(Device("default".to_owned()))
}
