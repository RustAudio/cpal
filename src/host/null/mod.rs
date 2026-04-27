//! Null backend implementation.
//!
//! Fallback no-op backend for unsupported platforms.

use std::time::Duration;

use crate::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Data, DeviceDescription, DeviceDescriptionBuilder, DeviceId, Error, FrameCount,
    InputCallbackInfo, OutputCallbackInfo, SampleFormat, StreamConfig, StreamInstant,
    SupportedStreamConfig, SupportedStreamConfigRange,
};

#[derive(Default)]
pub struct Devices;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Device;

pub struct Host;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stream;

// Compile-time assertion that Stream is Send and Sync
crate::assert_stream_send!(Stream);
crate::assert_stream_sync!(Stream);

#[derive(Clone)]
pub struct SupportedInputConfigs;
#[derive(Clone)]
pub struct SupportedOutputConfigs;

impl Host {
    #[allow(dead_code)]
    pub fn new() -> Result<Self, Error> {
        Ok(Self)
    }
}

impl Devices {
    pub fn new() -> Result<Self, Error> {
        Ok(Self)
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Ok(DeviceDescriptionBuilder::new("Null Device").build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId(crate::platform::HostId::Null, String::new()))
    }

    fn supported_input_configs(&self) -> Result<SupportedInputConfigs, Error> {
        unimplemented!()
    }

    fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        unimplemented!()
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        unimplemented!()
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        unimplemented!()
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
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        unimplemented!()
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
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        unimplemented!()
    }

    fn get_channel_name(&self, channel_index: u16, input: bool) -> Result<String, Error> {
        Err(Error::UnsupportedOperation)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        false
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        None
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        None
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        unimplemented!()
    }

    fn pause(&self) -> Result<(), Error> {
        unimplemented!()
    }

    fn now(&self) -> StreamInstant {
        unimplemented!()
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        unimplemented!()
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
