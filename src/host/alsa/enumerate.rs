use super::alsa;
use super::{Device, DeviceHandles};
use crate::{BackendSpecificError, DevicesError};
use std::sync::{Arc, Mutex};

/// ALSA's implementation for `Devices`.
pub struct Devices {
    builtin_pos: usize,
    card_iter: alsa::card::Iter,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices {
            builtin_pos: 0,
            card_iter: alsa::card::Iter::new(),
        })
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

const BUILTINS: [&'static str; 5] = ["default", "pipewire", "pulse", "jack", "oss"];

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        while self.builtin_pos < BUILTINS.len() {
            let pos = self.builtin_pos;
            self.builtin_pos += 1;
            let name = BUILTINS[pos];

            if let Ok(handles) = DeviceHandles::open(&name) {
                return Some(Device {
                    name: name.to_string(),
                    pcm_id: name.to_string(),
                    handles: Arc::new(Mutex::new(handles)),
                });
            }
        }

        loop {
            let Some(res) = self.card_iter.next() else {
                return None;
            };
            let Ok(card) = res else { continue };

            let ctl_id = format!("hw:{}", card.get_index());
            let Ok(ctl) = alsa::Ctl::new(&ctl_id, false) else {
                continue;
            };
            let Ok(cardinfo) = ctl.card_info() else {
                continue;
            };
            let Ok(card_name) = cardinfo.get_name() else {
                continue;
            };

            // Using plughw adds the ALSA plug layer, which can do sample type conversion,
            // sample rate convertion, ...
            // It is convenient, but at the same time not suitable for pro-audio as it hides
            // the actual device capabilities and perform audio manipulation under your feet,
            // for example sample rate conversion, sample format conversion, adds dummy channels,
            // ...
            // For now, many hardware only support 24bit / 3 bytes, which isn't yet supported by
            // cpal. So we have to enable plughw (unfortunately) for maximum compatibility.
            const USE_PLUGHW: bool = true;
            let pcm_id = if USE_PLUGHW {
                format!("plughw:{}", card.get_index())
            } else {
                ctl_id
            };
            if let Ok(handles) = DeviceHandles::open(&pcm_id) {
                return Some(Device {
                    name: card_name.to_string(),
                    pcm_id: pcm_id.to_string(),
                    handles: Arc::new(Mutex::new(handles)),
                });
            }
        }
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
