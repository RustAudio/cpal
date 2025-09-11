pub use self::device::{
    default_input_device, default_output_device, Device, Devices, SupportedInputConfigs,
    SupportedOutputConfigs,
};
pub use self::stream::Stream;

/// Configuration for WASAPI streams.
#[derive(Clone, Debug, Default)]
pub struct WasapiStreamConfig {
    /// Whether to use exclusive mode (true) or shared mode (false, default).
    pub exclusive_mode: Option<bool>,
}

impl WasapiStreamConfig {
    /// Create a new WASAPI stream configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set exclusive mode. When true, the stream will attempt to use exclusive mode
    /// which can provide lower latency but prevents other applications from using the device.
    pub fn exclusive_mode(mut self, exclusive: bool) -> Self {
        self.exclusive_mode = Some(exclusive);
        self
    }
}
use crate::traits::HostTrait;
use crate::BackendSpecificError;
use crate::DevicesError;
use std::io::Error as IoError;
use windows::Win32::Media::Audio;

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

impl From<windows::core::Error> for BackendSpecificError {
    fn from(error: windows::core::Error) -> Self {
        BackendSpecificError {
            description: format!("{}", IoError::from(error)),
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

fn windows_err_to_cpal_err<E: ErrDeviceNotAvailable>(e: windows::core::Error) -> E {
    windows_err_to_cpal_err_message::<E>(e, "")
}

fn windows_err_to_cpal_err_message<E: ErrDeviceNotAvailable>(
    e: windows::core::Error,
    message: &str,
) -> E {
    match e.code() {
        Audio::AUDCLNT_E_DEVICE_INVALIDATED => E::device_not_available(),
        _ => {
            let description = format!("{}{}", message, e);
            let err = BackendSpecificError { description };
            err.into()
        }
    }
}
