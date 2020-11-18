use super::alsa;
use super::parking_lot::Mutex;
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
                    let playback = alsa::pcm::PCM::new(&name, alsa::Direction::Playback, true).ok();

                    // See if the device has an available input stream.
                    let capture = alsa::pcm::PCM::new(&name, alsa::Direction::Capture, true).ok();

                    if playback.is_some() || capture.is_some() {
                        return Some(Device {
                            name,
                            handles: Mutex::new(super::DeviceHandles { playback, capture }),
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
        handles: Mutex::new(super::DeviceHandles {
            playback: None,
            capture: None,
        }),
    })
}

#[inline]
pub fn default_output_device() -> Option<Device> {
    Some(Device {
        name: "default".to_owned(),
        handles: Mutex::new(super::DeviceHandles {
            playback: None,
            capture: None,
        }),
    })
}

impl From<alsa::Error> for DevicesError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}
