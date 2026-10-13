//! WASAPI backend implementation.
//!
//! Default backend on Windows.

use std::io::Error as IoError;

use windows::Win32::Media::Audio;

#[allow(unused_imports)]
pub use self::device::{
    default_input_device, default_output_device, Device, Devices, SupportedInputConfigs,
    SupportedOutputConfigs,
};
#[allow(unused_imports)]
pub use self::stream::Stream;
use crate::{traits::HostTrait, Error, ErrorKind};

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
    pub fn new() -> Result<Self, Error> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume WASAPI is always available on Windows.
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
}

impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        let kind = match e.code() {
            Audio::AUDCLNT_E_SERVICE_NOT_RUNNING => ErrorKind::HostUnavailable,

            Audio::AUDCLNT_E_DEVICE_INVALIDATED | Audio::AUDCLNT_E_ENDPOINT_CREATE_FAILED => {
                ErrorKind::DeviceNotAvailable
            }

            Audio::AUDCLNT_E_DEVICE_IN_USE => ErrorKind::DeviceBusy,

            Audio::AUDCLNT_E_RESOURCES_INVALIDATED => ErrorKind::StreamInvalidated,

            Audio::AUDCLNT_E_UNSUPPORTED_FORMAT
            | Audio::AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED
            | Audio::AUDCLNT_E_BUFFER_SIZE_ERROR
            | Audio::AUDCLNT_E_INVALID_DEVICE_PERIOD
            | Audio::AUDCLNT_E_EXCLUSIVE_MODE_ONLY
            | Audio::AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED => ErrorKind::UnsupportedConfig,

            Audio::AUDCLNT_E_WRONG_ENDPOINT_TYPE
            | Audio::AUDCLNT_E_ALREADY_INITIALIZED
            | Audio::AUDCLNT_E_NOT_INITIALIZED
            | Audio::AUDCLNT_E_NOT_STOPPED => ErrorKind::UnsupportedOperation,

            _ => ErrorKind::BackendError,
        };
        Error::with_message(kind, IoError::from(e).to_string())
    }
}
