use crate::traits::*;
use crate::*;
use r#impl::{Device, DeviceIter, Stream, StreamDirectionalArgs, SupportedConfigs};
use std::fmt;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use tap::prelude::*;
use utils::*;

#[macro_use]
mod utils;
pub mod r#impl;

#[expect(
    clippy::derived_hash_with_manual_eq,
    reason = "this will never collide"
)]
#[derive(Debug, Hash)]
pub struct Handle<T>(Arc<T>);

impl<T> Handle<T> {
    fn new(inner: T) -> Self {
        Self(inner.into())
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        self.0
            .pipe_ref(Arc::clone)
            .pipe(Self)
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> Eq for Handle<T> {}

////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////

// no need to wrap the Host in `Handle`, since the underlying
// mpsc channel happens to already fulfill the same purpose
pub use r#impl::Host;

impl HostTrait for Host {
    type Device = Handle<Device>;
    type Devices = DeviceHandleIter;

    fn is_available() -> bool {
        Self::is_available_()
    }

    fn devices(&self) -> CpalResult<Self::Devices> {
        self.devices_().map(DeviceHandleIter)
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.default_device_(Device::supports_direction::<true, false>)
            .map(Handle::new)
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.default_device_(Device::supports_direction::<false, true>)
            .map(Handle::new)
    }

    fn device_by_id(&self, id: &DeviceId) -> Option<Self::Device> {
        self.device_by_id_(id)
            .map(Handle::new)
    }
}

#[derive(Debug, Clone)]
pub struct DeviceHandleIter(DeviceIter);

impl Iterator for DeviceHandleIter {
    type Item = Handle<Device>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(Handle::new)
    }
}

////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////

impl fmt::Display for Handle<Device> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_(f)
    }
}

impl DeviceTrait for Handle<Device> {
    type SupportedInputConfigs = SupportedConfigs;
    type SupportedOutputConfigs = SupportedConfigs;
    type Stream = Handle<Stream>;

    fn description(&self) -> CpalResult<DeviceDescription> {
        self.0.description()
    }

    fn id(&self) -> CpalResult<DeviceId> {
        self.0.id()
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
        self.0
            .build_stream_raw(
                move |data, _, cbi| data_cb(data, &cbi.output()),
                error_cb,
                timeout,
                config.sample_rate,
                config.buffer_size,
                [
                    Some(StreamDirectionalArgs { format, channels: config.channels }),
                    None
                ]
            )
            .map(Handle::<Stream>::new)
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
        self.0
            .build_stream_raw(
                move |_, data, cbi| data_cb(data, &cbi.output()),
                error_cb,
                timeout,
                config.sample_rate,
                config.buffer_size,
                [
                    None,
                    Some(StreamDirectionalArgs { format, channels: config.channels })
                ]
            )
            .map(Handle::<Stream>::new)
    }

    fn build_duplex_stream_raw<DataCb, ErrorCb>(
        &self,
        config    : DuplexStreamConfig,
        format_in : SampleFormat,
        format_out: SampleFormat,
        data_cb   : DataCb,
        error_cb  : ErrorCb,
        timeout   : Option<Duration>,
    ) -> CpalResult<Self::Stream>
    where
        DataCb: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        ErrorCb: FnMut(Error) + Send + 'static,
    {
        self.0
            .build_stream_raw(
                data_cb,
                error_cb,
                timeout,
                config.sample_rate,
                config.buffer_size,
                [
                    Some(StreamDirectionalArgs { format: format_in , channels: config. input_channels }),
                    Some(StreamDirectionalArgs { format: format_out, channels: config.output_channels })
                ]
            )
            .map(Handle::<Stream>::new)
    }
}

impl StreamTrait for Handle<Stream> {
    fn start(&self) -> CpalResult<()> {
        self.0.resume()
    }

    fn pause(&self) -> CpalResult<()> {
        self.0.halt()
    }

    fn stop(&self, _timeout: Option<Duration>) -> CpalResult<()> {
        self.0.halt()
    }

    fn now(&self) -> StreamInstant {
        self.0.now()
    }

    fn buffer_size(&self) -> CpalResult<FrameCount> {
        self.0.buffer_size()
    }
}
