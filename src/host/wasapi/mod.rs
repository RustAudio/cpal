extern crate winapi;

use BackendSpecificError;
use DevicesError;
use self::winapi::um::winnt::HRESULT;
use std::io::Error as IoError;
use traits::{HostTrait};
pub use self::device::{Device, Devices, SupportedInputFormats, SupportedOutputFormats, default_input_device, default_output_device};
pub use self::stream::Stream;

mod com;
mod device;
mod stream;

/// The WASAPI host, the default windows host type.
///
/// Note: If you use a WASAPI output device as an input device it will
/// transparently enable loopback mode (see
/// https://docs.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording).
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume WASAPI is always available on windows.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
}

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
        Ok(()) => Ok(()),
        Err(err) => {
            let description = format!("{}", err);
            return Err(BackendSpecificError { description });
        }
    }
}
