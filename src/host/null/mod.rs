//! Null backend implementation.
//!
//! Fallback no-op backend for unsupported platforms.

use std::fmt;
use std::time::Duration;

use crate::{
    CallbackInfo, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceId, Error,
    ErrorKind::DeviceNotAvailable,
    FrameCount, SampleFormat, StreamConfig, StreamInstant, SupportedStreamConfig,
    SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

pub struct Devices;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Device;

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = self.description().map_err(|_| fmt::Error)?;
        f.write_str(desc.name())
    }
}

pub struct Host;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stream;

#[derive(Clone)]
pub struct SupportedInputConfigs;
#[derive(Clone)]
pub struct SupportedOutputConfigs;

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Ok(DeviceDescriptionBuilder::new("Null Device").build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId::new(crate::platform::HostId::Null, ""))
    }

    fn supported_input_configs(&self) -> Result<SupportedInputConfigs, Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _config: StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    /// Create an output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        _config: StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn new() -> Result<Self, Error> {
        Ok(Self)
    }

    fn is_available() -> bool {
        false
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Ok(Devices)
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        None
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        None
    }
}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    fn pause(&self) -> Result<(), Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    fn stop(&self, _timeout: Option<std::time::Duration>) -> Result<(), Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }

    fn now(&self) -> StreamInstant {
        StreamInstant::ZERO
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Error::with_message(DeviceNotAvailable, "Null host in use")
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

impl Iterator for SupportedInputConfigs {
    type Item = SupportedStreamConfigRange;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

impl Iterator for SupportedOutputConfigs {
    type Item = SupportedStreamConfigRange;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}
