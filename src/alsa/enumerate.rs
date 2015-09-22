use super::alsa;
use super::check_errors;
use super::Endpoint;

use std::ffi::CStr;
use std::ffi::CString;
use std::mem;

use libc;

/// ALSA implementation for `EndpointsIterator`.
pub struct EndpointsIterator {
    // we keep the original list so that we can pass it to the free function
    global_list: *const *const u8,

    // pointer to the next string ; contained within `global_list`
    next_str: *const *const u8,
}

unsafe impl Send for EndpointsIterator {}
unsafe impl Sync for EndpointsIterator {}

impl Drop for EndpointsIterator {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            alsa::snd_device_name_free_hint(self.global_list as *mut _);
        }
    }
}

impl Default for EndpointsIterator {
    fn default() -> EndpointsIterator {
        unsafe {
            let mut hints = mem::uninitialized();
            // TODO: check in which situation this can fail
            check_errors(alsa::snd_device_name_hint(-1, b"pcm\0".as_ptr() as *const _,
                                                    &mut hints)).unwrap();

            let hints = hints as *const *const u8;

            EndpointsIterator {
                global_list: hints,
                next_str: hints,
            }
        }
    }
}

impl Iterator for EndpointsIterator {
    type Item = Endpoint;

    fn next(&mut self) -> Option<Endpoint> {
        loop {
            unsafe {
                if (*self.next_str).is_null() {
                    return None;
                }

                let name = {
                    let n_ptr = alsa::snd_device_name_get_hint(*self.next_str as *const _,
                                                               b"NAME\0".as_ptr() as *const _);
                    if !n_ptr.is_null() {
                        let n = CStr::from_ptr(n_ptr).to_bytes().to_vec();
                        let n = String::from_utf8(n).unwrap();
                        libc::free(n_ptr as *mut _);
                        Some(n)
                    } else {
                        None
                    }
                };

                let io = {
                    let n_ptr = alsa::snd_device_name_get_hint(*self.next_str as *const _,
                                                               b"IOID\0".as_ptr() as *const _);
                    if !n_ptr.is_null() {
                        let n = CStr::from_ptr(n_ptr).to_bytes().to_vec();
                        let n = String::from_utf8(n).unwrap();
                        libc::free(n_ptr as *mut _);
                        Some(n)
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

                if let Some(name) = name {
                    // trying to open the PCM device to see if it can be opened
                    let name_zeroed = CString::new(name.clone()).unwrap();
                    let mut playback_handle = mem::uninitialized();
                    if alsa::snd_pcm_open(&mut playback_handle, name_zeroed.as_ptr() as *const _,
                                          alsa::SND_PCM_STREAM_PLAYBACK, alsa::SND_PCM_NONBLOCK) == 0
                    {
                        alsa::snd_pcm_close(playback_handle);
                    } else {
                        continue;
                    }

                    // ignoring the `null` device
                    if name != "null" {
                        return Some(Endpoint(name));
                    }
                }
            }
        }
    }
}

#[inline]
pub fn get_default_endpoint() -> Option<Endpoint> {
    Some(Endpoint("default".to_owned()))
}
