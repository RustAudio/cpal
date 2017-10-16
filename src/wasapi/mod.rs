extern crate winapi;
extern crate ole32;
extern crate kernel32;

use std::io::Error as IoError;

pub use self::endpoint::{Endpoint, EndpointsIterator, default_endpoint, SupportedFormatsIterator};
pub use self::voice::{Buffer, EventLoop, VoiceId};

mod com;
mod endpoint;
mod voice;

#[inline]
fn check_result(result: winapi::HRESULT) -> Result<(), IoError> {
    if result < 0 {
        Err(IoError::from_raw_os_error(result))
    } else {
        Ok(())
    }
}
