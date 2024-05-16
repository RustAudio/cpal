use std::time::Duration;

pub use crate::host::wasapi::device::{SupportedInputConfigs, SupportedOutputConfigs};
use crate::traits::DeviceTrait;
use crate::traits::HostTrait;
use crate::traits::StreamTrait;
use crate::DevicesError;

use super::wasapi;
use super::wasapi::ShareMode;

/// The WASAPI exclusive host.
///
/// In exclusive mode only one stream can be opened per device, no mixing of multiple streams are performed.
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
        wasapi::Devices::new(ShareMode::Exclusive).map(Devices)
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        wasapi::default_input_device(ShareMode::Exclusive).map(Device)
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        wasapi::default_output_device(ShareMode::Exclusive).map(Device)
    }
}

pub struct Devices(wasapi::Devices);

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(Device)
    }
}

#[derive(Clone)]
pub struct Device(wasapi::Device);

impl DeviceTrait for Device {
    type SupportedInputConfigs = <wasapi::Device as DeviceTrait>::SupportedInputConfigs;
    type SupportedOutputConfigs = <wasapi::Device as DeviceTrait>::SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, crate::DeviceNameError> {
        self.0.name()
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, crate::SupportedStreamConfigsError> {
        self.0.supported_input_configs()
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, crate::SupportedStreamConfigsError> {
        self.0.supported_output_configs()
    }

    fn default_input_config(
        &self,
    ) -> Result<crate::SupportedStreamConfig, crate::DefaultStreamConfigError> {
        self.0.default_input_config()
    }

    fn default_output_config(
        &self,
    ) -> Result<crate::SupportedStreamConfig, crate::DefaultStreamConfigError> {
        self.0.default_output_config()
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &crate::StreamConfig,
        sample_format: crate::SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, crate::BuildStreamError>
    where
        D: FnMut(&crate::Data, &crate::InputCallbackInfo) + Send + 'static,
        E: FnMut(crate::StreamError) + Send + 'static,
    {
        self.0
            .build_input_stream_raw(
                config,
                sample_format,
                data_callback,
                error_callback,
                timeout,
            )
            .map(Stream)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &crate::StreamConfig,
        sample_format: crate::SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, crate::BuildStreamError>
    where
        D: FnMut(&mut crate::Data, &crate::OutputCallbackInfo) + Send + 'static,
        E: FnMut(crate::StreamError) + Send + 'static,
    {
        self.0
            .build_output_stream_raw(
                config,
                sample_format,
                data_callback,
                error_callback,
                timeout,
            )
            .map(Stream)
    }
}

pub struct Stream(wasapi::Stream);

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), crate::PlayStreamError> {
        self.0.play()
    }

    fn pause(&self) -> Result<(), crate::PauseStreamError> {
        self.0.pause()
    }
}
