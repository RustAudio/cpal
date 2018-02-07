extern crate winapi;

use std::io::Error as IoError;

pub use self::endpoint::{Endpoint, EndpointsIterator, SupportedFormatsIterator, default_endpoint};
pub use self::voice::{Buffer, EventLoop, VoiceId};
use self::winapi::um::winnt::HRESULT;

mod com;
mod endpoint;
mod voice;

#[inline]
fn check_result(result: HRESULT) -> Result<(), IoError> {
    if result < 0 {
        Err(IoError::from_raw_os_error(result))
    } else {
        Ok(())
    }
}
