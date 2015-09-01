extern crate libc;
extern crate winapi;
extern crate ole32;

use std::io::Error as IoError;

pub use self::enumerate::{EndpointsIterator, get_default_endpoint};
pub use self::voice::{Voice, Buffer};

mod com;
mod enumerate;
mod voice;

/// An opaque type that identifies an end point.
#[derive(PartialEq, Eq)]
#[allow(raw_pointer_derive)]
pub struct Endpoint(*mut winapi::IMMDevice);

unsafe impl Send for Endpoint {}
unsafe impl Sync for Endpoint {}

impl Clone for Endpoint {
    fn clone(&self) -> Endpoint {
        unsafe { (*self.0).AddRef(); }
        Endpoint(self.0)
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        unsafe { (*self.0).Release(); }
    }
}

fn check_result(result: winapi::HRESULT) -> Result<(), IoError> {
    if result < 0 {
        Err(IoError::from_raw_os_error(result))
    } else {
        Ok(())
    }
}
