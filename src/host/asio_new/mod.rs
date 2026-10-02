//! Experimental ASIO backend implementation.
//!
//! Available on Windows with the `asio-new` feature.

use self::utils::CpalResult;
use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::*;
use std::fmt;
use std::time::Duration;

mod utils;

#[derive(Debug, Clone)]
pub struct Host();

impl Host {
    /// Required by the `impl_platform_host!` macro
    pub fn new() -> CpalResult<Self> {
        todo!()
    }
}

impl HostTrait for Host {
    type Device = Device;
    type Devices = Devices;

    fn is_available() -> bool {
        todo!()
    }

    fn devices(&self) -> CpalResult<Self::Devices> {
        todo!()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        todo!()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        todo!()
    }

    fn device_by_id(&self, id: &DeviceId) -> Option<Self::Device> {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct Devices;

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub type SupportedConfigs = !; // todo

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Device();

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedConfigs;
    type SupportedOutputConfigs = SupportedConfigs;
    type Stream = Stream;

    fn description(&self) -> CpalResult<DeviceDescription> {
        todo!()
    }

    fn id(&self) -> CpalResult<DeviceId> {
        todo!()
    }

    fn supported_input_configs(&self) -> CpalResult<Self::SupportedInputConfigs> {
        todo!()
    }

    fn supported_output_configs(&self) -> CpalResult<Self::SupportedOutputConfigs> {
        todo!()
    }

    fn default_input_config(&self) -> CpalResult<SupportedStreamConfig> {
        todo!()
    }

    fn default_output_config(&self) -> CpalResult<SupportedStreamConfig> {
        todo!()
    }

    fn supports_input(&self) -> bool {
        todo!()
    }

    fn supports_output(&self) -> bool {
        todo!()
    }

    fn supports_duplex(&self) -> bool {
        todo!()
    }

    fn build_input_stream_raw<DataCb, ErrorCb>(
        &self,
        config     : StreamConfig,
        format     : SampleFormat,
        mut data_cb: DataCb,
        error_cb   : ErrorCb,
        timeout    : Option<Duration>,
    ) -> CpalResult<Self::Stream>
    where
        DataCb: FnMut(&Data, &CallbackInfo) + Send + 'static,
        ErrorCb: FnMut(Error) + Send + 'static,
    {
        let duplex_cfg = DuplexStreamConfig {
            input_channels : config.channels,
            output_channels: 0,
            sample_rate    : config.sample_rate,
            buffer_size    : config.buffer_size,
        };

        self.build_duplex_stream_raw(
            duplex_cfg,
            format,
            format,
            move |data, _, cbi| data_cb(data, &cbi.input()),
            error_cb,
            timeout,
        )
    }

    fn build_output_stream_raw<DataCb, ErrorCb>(
        &self,
        config: StreamConfig,
        format: SampleFormat,
        mut data_cb: DataCb,
        error_cb: ErrorCb,
        timeout: Option<Duration>,
    ) -> CpalResult<Self::Stream>
    where
        DataCb: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        ErrorCb: FnMut(Error) + Send + 'static,
    {
        let duplex_cfg = DuplexStreamConfig {
            input_channels : 0,
            output_channels: config.channels,
            sample_rate    : config.sample_rate,
            buffer_size    : config.buffer_size,
        };

        self.build_duplex_stream_raw(
            duplex_cfg,
            format,
            format,
            move |_, data, cbi| data_cb(data, &cbi.output()),
            error_cb,
            timeout,
        )
    }

    fn build_duplex_stream_raw<DataCb, ErrorCb>(
        &self,
        config: DuplexStreamConfig,
        format_in : SampleFormat,
        format_out: SampleFormat,
        data_cb   : DataCb,
        error_cb  : ErrorCb,
        _timeout  : Option<Duration>,
    ) -> CpalResult<Self::Stream>
    where
        DataCb: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        ErrorCb: FnMut(Error) + Send + 'static,
    {
        todo!()
    }
}

#[derive(Debug)]
pub struct Stream;

impl StreamTrait for Stream {
    fn start(&self) -> CpalResult<()> {
        todo!()
    }

    fn pause(&self) -> CpalResult<()> {
        todo!()
    }

    fn stop(&self, timeout: Option<Duration>) -> CpalResult<()> {
        todo!()
    }

    fn now(&self) -> StreamInstant {
        todo!()
    }

    fn buffer_size(&self) -> CpalResult<FrameCount> {
        todo!()
    }
}
