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
                    let name = hint.name;

                    let io = hint.direction;

                    if let Some(io) = io {
                        if io != alsa::Direction::Playback {
                            continue;
                        }
                    }

                    let name = match name {
                        Some(name) => {
                            // Ignoring the `null` device.
                            if name == "null" {
                                continue;
                            }
                            name
                        }
                        _ => continue,
                    };

                    // See if the device has an available output stream.
                    let has_available_output = {
                        let playback_handle =
                            alsa::pcm::PCM::new(&name, alsa::Direction::Playback, true);
                        playback_handle.is_ok()
                    };

                    // See if the device has an available input stream.
                    let has_available_input = {
                        let capture_handle =
                            alsa::pcm::PCM::new(&name, alsa::Direction::Capture, true);
                        capture_handle.is_ok()
                    };

                    if has_available_output || has_available_input {
                        return Some(Device(name));
                    }
                }
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

impl From<alsa::Error> for DevicesError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}
