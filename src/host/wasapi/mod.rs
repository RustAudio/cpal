pub use self::device::{
    default_input_device, default_output_device, Device, Devices, SupportedInputConfigs,
    SupportedOutputConfigs,
};
pub use self::stream::Stream;
use windows::core::HRESULT;
use windows::Win32::Media::Audio;
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

#[inline]
fn check_result(result: HRESULT) -> Result<(), IoError> {
    if result.is_err() {
        Err(IoError::from_raw_os_error(result.0))
    } else {
        Ok(())
    }
}

impl From<windows::core::Error> for BackendSpecificError {
    fn from(error: windows::core::Error) -> Self {
	BackendSpecificError {
	    description: format!("{}", IoError::from(error))
	}
    }
}

fn check_result_backend_specific(result: HRESULT) -> Result<(), BackendSpecificError> {
    match check_result(result) {
        Ok(()) => Ok(()),
        Err(err) => Err(BackendSpecificError {
            description: format!("{}", err),
        }),
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

// TODO: This isn't just used for audioclient, so rename it
// appropriately.
fn audioclient_error<E: ErrDeviceNotAvailable>(e: windows::core::Error) -> E {
    audioclient_error_message::<E>(e, "")
}

fn audioclient_error_message<E: ErrDeviceNotAvailable>(e: windows::core::Error, message: &str) -> E {
    match e.code() {
        Audio::AUDCLNT_E_DEVICE_INVALIDATED => {
	    E::device_not_available()
        }
        _ => {
            let description = format!("{}{}", message, e);
            let err = BackendSpecificError { description };
            err.into()
        }
    }
}
