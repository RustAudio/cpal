extern crate winapi;

use BackendSpecificError;
use self::winapi::um::winnt::HRESULT;
use std::io::Error as IoError;
pub use self::device::{Device, Devices, SupportedInputFormats, SupportedOutputFormats, default_input_device, default_output_device};
pub use self::stream::{EventLoop, StreamId};

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

fn check_result_backend_specific(result: HRESULT) -> Result<(), BackendSpecificError> {
    match check_result(result) {
        Ok(()) => Ok(())
        Err(err) => {
            let description = format!("{}", err);
            return BackendSpecificError { description }
        }
    }
}
