//! Experimental ASIO backend implementation.
//!
//! Available on Windows with the `asio-new` feature.

use self::utils::CpalResult;
use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::*;
use std::fmt;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tap::prelude::*;
use self::driver::Session;
mod utils;
mod driver;

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

#[derive(Debug)]
pub struct Device(Arc<Session>);

impl Clone for Device {
    fn clone(&self) -> Self {
        self.0
            .pipe_ref(Arc::clone)
            .pipe(Self)
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Device {}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.display_name())
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedConfigs;
    type SupportedOutputConfigs = SupportedConfigs;
    type Stream = Stream;

    fn description(&self) -> CpalResult<DeviceDescription> {
        self.0.description()
    }

    fn id(&self) -> CpalResult<DeviceId> {
        self.0.id().pipe(Ok)
    }

    fn supported_input_configs(&self) -> CpalResult<Self::SupportedInputConfigs> {
        self.0.supported_configs::<true>()
    }

    fn supported_output_configs(&self) -> CpalResult<Self::SupportedOutputConfigs> {
        self.0.supported_configs::<false>()
    }

    fn default_input_config(&self) -> CpalResult<SupportedStreamConfig> {
        self.0.default_config::<true>()
    }

    fn default_output_config(&self) -> CpalResult<SupportedStreamConfig> {
        self.0.default_config::<false>()
    }

    fn supports_input(&self) -> bool {
        self.0.supports_direction::<true, false>()
    }

    fn supports_output(&self) -> bool {
        self.0.supports_direction::<false, true>()
    }

    fn supports_duplex(&self) -> bool {
        self.0.supports_direction::<true, true>()
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
pub struct Stream {
    session: Arc<Session>,
}

impl StreamTrait for Stream {
    fn start(&self) -> CpalResult<()> {
        self.session.start()
    }

    fn pause(&self) -> CpalResult<()> {
        self.session.pause()
    }

    fn stop(&self, timeout: Option<Duration>) -> CpalResult<()> {
        self.session.stop(timeout)
    }

    fn now(&self) -> StreamInstant {
        self.session.now()
    }

    fn buffer_size(&self) -> CpalResult<FrameCount> {
        todo!()
    }
}
