use super::alsa;
use super::check_errors;
use super::Device;
use std::ffi::CString;
use std::ptr;
use {BackendSpecificError, DevicesError};

/// ALSA implementation for `Devices`.
pub struct Devices {
    // we keep the original list so that we can pass it to the free function
    global_list: *const *const u8,

    // pointer to the next string ; contained within `global_list`
    next_str: *const *const u8,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        unsafe {
            // TODO: check in which situation this can fail.
            let card = -1; // -1 means all cards.
            let iface = b"pcm\0"; // Interface identification.
            let mut hints = ptr::null_mut(); // Array of device name hints.
            let res = alsa::snd_device_name_hint(card, iface.as_ptr() as *const _, &mut hints);
            if let Err(description) = check_errors(res) {
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            let hints = hints as *const *const u8;
            let devices = Devices {
                global_list: hints,
                next_str: hints,
            };
            Ok(devices)
        }
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

impl Drop for Devices {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            alsa::snd_device_name_free_hint(self.global_list as *mut _);
        }
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        loop {
            unsafe {
                if (*self.next_str).is_null() {
                    return None;
                }

                let name = {
                    let n_ptr = alsa::snd_device_name_get_hint(
                        *self.next_str as *const _,
                        b"NAME\0".as_ptr() as *const _,
                    );
                    if !n_ptr.is_null() {
                        let bytes = CString::from_raw(n_ptr).into_bytes();
                        let string = String::from_utf8(bytes).unwrap();
                        Some(string)
                    } else {
                        None
                    }
                };

                let io = {
                    let n_ptr = alsa::snd_device_name_get_hint(
                        *self.next_str as *const _,
                        b"IOID\0".as_ptr() as *const _,
                    );
                    if !n_ptr.is_null() {
                        let bytes = CString::from_raw(n_ptr).into_bytes();
                        let string = String::from_utf8(bytes).unwrap();
                        Some(string)
                    } else {
                        None
                    }
                };

                self.next_str = self.next_str.offset(1);

                if let Some(io) = io {
                    if io != "Output" {
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

                // trying to open the PCM device to see if it can be opened
                let name_zeroed = CString::new(&name[..]).unwrap();

                // See if the device has an available output stream.
                let mut playback_handle = ptr::null_mut();
                let has_available_output = alsa::snd_pcm_open(
                    &mut playback_handle,
                    name_zeroed.as_ptr() as *const _,
                    alsa::SND_PCM_STREAM_PLAYBACK,
                    alsa::SND_PCM_NONBLOCK,
                ) == 0;
                if has_available_output {
                    alsa::snd_pcm_close(playback_handle);
                }

                // See if the device has an available input stream.
                let mut capture_handle = ptr::null_mut();
                let has_available_input = alsa::snd_pcm_open(
                    &mut capture_handle,
                    name_zeroed.as_ptr() as *const _,
                    alsa::SND_PCM_STREAM_CAPTURE,
                    alsa::SND_PCM_NONBLOCK,
                ) == 0;
                if has_available_input {
                    alsa::snd_pcm_close(capture_handle);
                }

                if has_available_output || has_available_input {
                    return Some(Device(name));
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
