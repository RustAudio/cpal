pub use self::device::{
    default_input_device, default_output_device, Device, Devices, SupportedInputConfigs,
    SupportedOutputConfigs,
};
pub use self::stream::Stream;
use crate::traits::HostTrait;
use crate::BackendSpecificError;
use crate::DevicesError;
use std::io::Error as IoError;

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
        // Assume WASAPI is always available on Windows.
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

impl From<i32> for BackendSpecificError {
    fn from(error: i32) -> Self {
        BackendSpecificError {
            description: format!("{}", IoError::from_raw_os_error(error)),
        }
    }
}

trait ErrDeviceNotAvailable: From<BackendSpecificError> {
    fn device_not_available() -> Self;
}

impl ErrDeviceNotAvailable for crate::BuildStreamError {
    fn device_not_available() -> Self {
        Self::DeviceNotAvailable
    }
}

impl ErrDeviceNotAvailable for crate::SupportedStreamConfigsError {
    fn device_not_available() -> Self {
        Self::DeviceNotAvailable
    }
}

impl ErrDeviceNotAvailable for crate::DefaultStreamConfigError {
    fn device_not_available() -> Self {
        Self::DeviceNotAvailable
    }
}

impl ErrDeviceNotAvailable for crate::StreamError {
    fn device_not_available() -> Self {
        Self::DeviceNotAvailable
    }
}

#[inline]
fn windows_err_to_cpal_err<E: ErrDeviceNotAvailable>(e: ::windows_core::Error) -> E {
    windows_err_to_cpal_err_message(e, "")
}

#[inline]
fn windows_err_to_cpal_err_message<E: ErrDeviceNotAvailable>(
    e: ::windows_core::Error,
    message: &str,
) -> E {
    if let com::bindings::AUDCLNT_E_DEVICE_INVALIDATED = e.code() {
        E::device_not_available()
    } else {
        let description = format!("{}{}", message, e);
        let err = BackendSpecificError { description };
        err.into()
    }
}
