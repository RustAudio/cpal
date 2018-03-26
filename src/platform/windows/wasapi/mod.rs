extern crate winapi;

use std::io::Error as IoError;

pub use self::device::{Device, Devices, SupportedInputFormats, SupportedOutputFormats, default_input_device, default_output_device};
pub use self::stream::{InputBuffer, OutputBuffer, EventLoop, StreamId};
use self::winapi::um::winnt::HRESULT;

mod com;
mod device;
mod stream;

#[inline]
fn check_result(result: HRESULT) -> Result<(), IoError> {
    if result < 0 {
        Err(IoError::from_raw_os_error(result))
    } else {
        Ok(())
    }
}
