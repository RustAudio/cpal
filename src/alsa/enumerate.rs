use super::alsa;
use super::check_errors;
use super::Endpoint;

use std::ffi::CStr;
use std::mem;

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

                let name = alsa::snd_device_name_get_hint(*self.next_str as *const _,
                                                          b"NAME".as_ptr() as *const _);
                self.next_str = self.next_str.offset(1);

                if name.is_null() {
                    continue;
                }

                let name = CStr::from_ptr(name).to_bytes().to_vec();
                let name = String::from_utf8(name).unwrap();

                if name != "null" {
                    return Some(Endpoint(name));
                }
            }
        }
    }
}

#[inline]
pub fn get_default_endpoint() -> Option<Endpoint> {
    // TODO: do in a different way?
    Some(Endpoint("default".to_owned()))
}
