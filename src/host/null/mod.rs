use std::time::Duration;

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat,
    StreamConfig, StreamError, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

#[derive(Default)]
pub struct Devices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Host;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stream;

pub struct SupportedInputConfigs;
pub struct SupportedOutputConfigs;

impl Host {
    #[allow(dead_code)]
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices)
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("null".to_owned())
    }

    #[inline]
    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        unimplemented!()
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        unimplemented!()
    }

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        unimplemented!()
    }

    #[inline]
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        unimplemented!()
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _config: &StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        unimplemented!()
    }

    /// Create an output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        _config: &StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        unimplemented!()
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        false
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Device> {
        None
    }

    fn default_output_device(&self) -> Option<Device> {
        None
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        unimplemented!()
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        unimplemented!()
    }
}

impl Iterator for Devices {
    type Item = Device;

    #[inline]
    fn next(&mut self) -> Option<Device> {
        None
    }
}

impl Iterator for SupportedInputConfigs {
    type Item = SupportedStreamConfigRange;

    #[inline]
    fn next(&mut self) -> Option<SupportedStreamConfigRange> {
        None
    }
}

impl Iterator for SupportedOutputConfigs {
    type Item = SupportedStreamConfigRange;

    #[inline]
    fn next(&mut self) -> Option<SupportedStreamConfigRange> {
        None
    }
}
