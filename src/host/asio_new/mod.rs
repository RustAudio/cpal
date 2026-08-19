//! Experimental ASIO backend implementation.
//!
//! Available on Windows with the `asio-new` feature.

use crate::ErrorKind::*;
use crate::traits::*;
use crate::*;
use std::fmt;
use std::fmt::Debug;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::vec;
use tap::prelude::*;

#[macro_use]
mod utils;
mod callbacks;
mod capabilities;
mod session;
mod simplex;

use self::callbacks::Callbacks;
use self::session::Session;
use self::utils::*;

#[derive(Debug, Clone)]
pub struct Host(Arc<session::Factory>);

impl Host {
    /// Required by the `impl_platform_host!` macro
    pub fn new() -> CpalResult<Self> {
        session::Factory
            ::new()
            .pipe(Arc::new)
            .pipe(Self)
            .pipe(Ok)
    }
}

impl HostTrait for Host {
    type Device = Device;
    type Devices = Devices;

    fn is_available() -> bool {
        // this will return false if the ASIO registry keys are either
        // * missing - meaning no ASIO driver has ever been installed on the system
        // * corrupted - in which case ASIO is unusable
        azo::get_drivers().is_ok()
    }

    fn devices(&self) -> CpalResult<Self::Devices> {
        self.0
            .pipe_ref(Arc::clone)
            .pipe(Devices::new)
            .map_err(|win_error| Error::with_message(HostUnavailable, win_error.message()))
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.devices()
            .ok()?
            .into_iter()
            .find(Device::supports_input)
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.devices()
            .ok()?
            .into_iter()
            .find(Device::supports_output)
    }

    fn device_by_id(&self, id: &DeviceId) -> Option<Self::Device> {
        if id.host() != HostId::AsioNew {
            return None;
        }

        let clsid = id.id().try_into().ok()?;

        self.0
            .get_session(&clsid)
            .ok()
            .map(Device)
    }
}

#[derive(Debug, Clone)]
pub struct Devices(Arc<session::Factory>, vec::IntoIter<azo::DriverMetadata>);

impl Devices {
    pub fn new(factory: Arc<session::Factory>) -> azo::WinResult<Self> {
        let metas = azo::get_drivers()?.into_iter();

        Ok(Self(factory, metas))
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.1
            .find_map(|metadata|
                self.0
                    .get_session(&metadata.clsid)
                    .ok()
            )
            .map(Device)
    }
}

pub type SupportedConfigs = vec::IntoIter<SupportedStreamConfigRange>;

#[expect(
    clippy::derived_hash_with_manual_eq,
    reason = "manual eq is more strict"
)]
#[derive(Debug, Hash)]
pub struct Device(Arc<Session>);

impl Device {
    fn new(session: Session) -> Self {
        session
            .pipe(Arc::new)
            .pipe(Self)
    }
}

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

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.name())
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
        self.supports_input() && self.supports_output()
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
            buffer_size    : config.buffer_size
        };

        self.build_duplex_stream_raw(
            duplex_cfg,
            format,
            format,
            move |data, _, cbi| data_cb(data, &cbi.input()),
            error_cb,
            timeout
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
            buffer_size    : config.buffer_size
        };

        self.build_duplex_stream_raw(
            duplex_cfg,
            format,
            format,
            move |_, data, cbi| data_cb(data, &cbi.output()),
            error_cb,
            timeout
        )
    }

    fn build_duplex_stream_raw<DataCb, ErrorCb>(
        &self,
        DuplexStreamConfig { input_channels, output_channels, sample_rate, buffer_size }: DuplexStreamConfig,
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
        let cfg_in  = simplex::Config { format: format_in , channels: input_channels , input: true  };
        let cfg_out = simplex::Config { format: format_out, channels: output_channels, input: false };

        Session::build_stream(&self.0, cfg_in, cfg_out, sample_rate, buffer_size, data_cb, error_cb)
    }
}

#[derive(Debug)]
pub struct Stream {
    session    : Arc<Session>,
    frame_count: FrameCount,
    _callbacks : Pin<Box<Callbacks>>
}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl StreamTrait for Stream {
    fn start(&self) -> CpalResult<()> {
        self.session.start()
    }

    fn pause(&self) -> CpalResult<()> {
        self.session.pause()
    }

    fn stop(&self, _timeout: Option<Duration>) -> CpalResult<()> {
        self.session.stop()
    }

    fn now(&self) -> StreamInstant {
        self.session.now()
    }

    fn buffer_size(&self) -> CpalResult<FrameCount> {
        Ok(self.frame_count)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.session.reset();
    }
}
