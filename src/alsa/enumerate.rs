use super::alsa;
use super::check_errors;
use super::Endpoint;

use cpal_impl::libc::c_int;
use std::ffi::{CString};
use std::ptr::{null_mut};

/// ALSA implementation for `EndpointsIterator`.
pub struct EndpointsIterator {
    // current sound card number
    card:   c_int,

    // current sound card name, e.g. hw:0
    cardname:   String,

    // current sound card handle
    card_handle: *mut alsa::snd_ctl_t,

    // current sound device number
    dev:    c_int

}

impl EndpointsIterator {

    unsafe fn close_card_handle(&mut self) {
        if !self.card_handle.is_null() {
            check_errors(alsa::snd_ctl_close(self.card_handle)).unwrap();
            self.card_handle = null_mut();
        }
    }

    unsafe fn open_card_handle(&mut self) {
        self.cardname = format!("hw:{}", self.card);
        check_errors(alsa::snd_ctl_open(&mut self.card_handle, 
            CString::new(self.cardname.clone()).unwrap().as_ptr() as *const _, 0)).unwrap();
    }
    
    fn next_card(&mut self) -> bool
    {
        unsafe {
            self.close_card_handle();
            check_errors(alsa::snd_card_next(&mut self.card)).unwrap();
            if self.card >= 0 {
                self.open_card_handle();
                self.dev = -1;
                self.next_dev()
            } else
            {
                false
            }
        }
    }

    fn next_dev(&mut self) -> bool
    {
        unsafe {
            check_errors(alsa::snd_ctl_pcm_next_device(self.card_handle, &mut self.dev)).unwrap();
            self.dev >= 0
        }
    }

}

unsafe impl Send for EndpointsIterator {}
unsafe impl Sync for EndpointsIterator {}

impl Drop for EndpointsIterator {
    fn drop(&mut self) {
        unsafe { 
            self.close_card_handle();
        }
    }
}

impl Default for EndpointsIterator {
    fn default() -> EndpointsIterator {
        
        let mut endpoint = EndpointsIterator {
            card: -1,
            cardname: String::new(),
            card_handle: null_mut(),
            dev: -1,
        };
        endpoint.next_card();
        endpoint
    }
}

impl Iterator for EndpointsIterator {
    type Item = Endpoint;

    fn next(&mut self) -> Option<Endpoint> {
        let endpoint;
        if self.card >= 0 {
            endpoint = Some(Endpoint(format!("hw:{},{}", self.card,self.dev)));
            if !self.next_dev() {
                self.next_card();
            }
        } else {
            endpoint = None
        }
        endpoint
    }
}

pub fn get_default_endpoint() -> Option<Endpoint> {
    EndpointsIterator::default().next() // TODO: Find default device
}
