//! Experimental ASIO backend implementation.
//!
//! Available on Windows with the `asio-new` feature.

use super::com::worker;
use crate::ErrorKind::*;
use crate::traits::*;
use crate::*;
use azo::Driver;
use azo::dto::ChannelCounts;
use std::fmt;
use std::fmt::Debug;
use std::hash::Hash;
use std::hash::Hasher;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::vec;
use tap::prelude::*;
use windows_core::{GUID, Interface};

#[macro_use]
mod utils;
mod capabilities;
mod enumerate;
mod callbacks;
mod simplex;

use self::enumerate::{Devices, Sessions, SupportedConfigs};
use self::callbacks::Container;
use self::utils::*;

#[derive(Debug, Clone)]
pub struct Host(worker::Handle);

impl Host {
    /// Required by the `impl_platform_host!` macro
    pub fn new() -> CpalResult<Self> {
        worker::Handle
            ::new()
            .pipe(Self)
            .pipe(Ok)
    }

    fn sessions(&self) -> CpalResult<Sessions> {
        self.0
            .clone()
            .pipe(Sessions::new)
            .map_err(|_win_error| Error::new(HostUnavailable))
    }

    #[must_use]
    fn default_session<const INPUT: bool, const OUTPUT: bool>(&self) -> Option<Session> {
        self.sessions()
            .ok()?
            .find(Session::supports_direction::<INPUT, OUTPUT>)
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
        self.sessions()
            .map(Devices)
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.default_session::<true, false>()
            .map(Device::new)
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.default_session::<false, true>()
            .map(Device::new)
    }

    fn device_by_id(&self, id: &DeviceId) -> Option<Self::Device> {
        if id.host() != HostId::AsioNew {
            return None;
        }

        let clsid = id.id().try_into().ok()?;

        Session::try_new(clsid, &self.0)
            .ok()
            .map(Device::new)
    }
}

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
        f.write_str(&self.0.driver.name().to_string_lossy())
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
        
        self.0.set_sample_rate(sample_rate)?;
        let frame_count = self.0.get_frame_count(buffer_size)?;
        let callbacks = self.0.prepare(cfg_in, cfg_out, frame_count, data_cb, error_cb)?;

        Stream {
            session: Arc::clone(&self.0),
            frame_count,
            _callbacks: callbacks // keep this alive until the whole stream is dropped
        }
        .pipe(Ok)
    }
}

#[derive(Debug)]
pub struct Session {
    driver      : Driver,
    init_success: bool,
    clsid_string: String,
    _com_worker : worker::Handle
}

impl Session {
    fn try_new(clsid: GUID, com_worker: &worker::Handle) -> azo::WinResult<Self> {
        let driver = com_worker.create_driver(clsid)?;

        Self {
            init_success: driver.init(None),
            driver,
            clsid_string: format!("{clsid:?}"),
            _com_worker: com_worker.clone(), // hold on to this to keep the thread alive that initialized the COM apartment in which the driver was created
        }
        .pipe(Ok)
    }

    fn id(&self) -> CpalResult<DeviceId> {
        DeviceId::new(
            HostId::AsioNew,
            self.clsid_string.clone()
        )
        .pipe(Ok)
    }

    fn description(&self) -> CpalResult<DeviceDescription> {
        let name_c = self.driver.name();
        let name = name_c.to_string_lossy();

        let direction = match capabilities::channel_counts(&self.driver)? {
            ChannelCounts { in_: 1.., out: 1.. } => DeviceDirection::Duplex,
            ChannelCounts { in_: 1.., out: 0   } => DeviceDirection::Input,
            ChannelCounts { in_: 0  , out: 1.. } => DeviceDirection::Output,
            _                                    => DeviceDirection::Unknown,
        };

        let mut extended = vec![format!("driver version: {}", self.driver.version())];

        if !self.init_success {
            extended.push("ASIO driver failed to initialize".to_owned()); // ASIO drivers can often still do *something* when they fail to initialize
            extended.push(format!("last error: {}", self.driver.last_error().to_string_lossy()));
        }

        DeviceDescriptionBuilder
            ::new(&name)
            .driver(name)
            .direction(direction)
            .extended(extended)
            .build()
            .pipe(Ok)
    }

    #[must_use]
    fn supports_direction<const IN: bool, const OUT: bool>(&self) -> bool {
        if !self.init_success {
            return false;
        }

        let Ok(counts) = self.driver.channel_counts()
        else { return false; }; // can't do anything if it can't even count the channels

        if IN && counts.in_ == 0 {
            return false;
        }

        if OUT && counts.out == 0 {
            return false;
        }

        true
    }

    fn supported_configs<const INPUT: bool>(&self) -> CpalResult<SupportedConfigs> {
        let ch_count = capabilities::channel_count::<INPUT>(&self.driver)?;
        if ch_count == 0 {
            return err(UnsupportedOperation, "the device has no channels in this direction");
        }

        let (min_rate, max_rate) = capabilities::sample_rates(&self.driver)?;
        let buf_size             = capabilities::buffer_size_supported(&self.driver);
        let sample_formats       = capabilities::sample_formats::<INPUT>(&self.driver, ch_count)?;

        sample_formats
            .map(move |format| SupportedStreamConfigRange::new(ch_count as _, min_rate, max_rate, buf_size, format))
            .collect::<Vec<_>>()
            .into_iter()
            .pipe(Ok)
    }

    fn default_config<const INPUT: bool>(&self) -> CpalResult<SupportedStreamConfig> {
        self.supported_configs::<INPUT>()?
            .next()
            .expect("infallible")
            .pipe(|range|
                SupportedStreamConfig::new(
                    range.channels(),
                    range.min_sample_rate(),
                    *range.buffer_size(),
                    range.sample_format()
                )
            )
            .pipe(Ok)
    }

    fn get_frame_count(&self, requested: BufferSize) -> CpalResult<FrameCount> {
        match requested {
            BufferSize::Fixed(n) => n,
            BufferSize::Default  => capabilities::buffer_size_preferred(&self.driver)? as FrameCount,
        }
        .pipe(Ok)
    }
    
    fn set_sample_rate(&self, sample_rate: SampleRate) -> CpalResult<()> {
        self.driver
            .can_sample_rate(sample_rate as _)
            .map_err(|_| Error::with_message(InvalidInput, "sample rate not supported"))?;

        self.driver
            .set_sample_rate(sample_rate as _)
            .map_err(|azo_error| create_report(&self.driver, azo_error, "set_sample_rate"))?;
        
        Ok(())
    }
    
    /// ASIO lifecycle stage 2 ("initialized") -> stage 3 ("prepared")
    /// - See ASIO specification section II.2
    fn prepare(
        self       : &Arc<Self>,
        cfg_in     : simplex::Config,
        cfg_out    : simplex::Config,
        frame_count: FrameCount,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!()
    ) -> CpalResult<Pin<Box<Container>>> {
        let channel_ids: Vec<_> = [cfg_in, cfg_out]
            .into_iter()
            .flat_map(|cfg| cfg.validate(&self.driver))
            .collect::<CpalResult<_>>()?;

        let mut callbacks: Pin<Box<Container>> = Default::default(); // creates dummy callbacks that do nothing
        
        let mut double_buffers =
            unsafe { self.driver.create_buffers(channel_ids, frame_count as _, callbacks.pointers()) }
            .map_err(|error| create_report(&self.driver, error, "create_buffers"))?
            .map(DoubleBuffer);
        
        let buffers_in  = double_buffers.by_ref().take(cfg_in.channels as _).collect();
        let buffers_out = double_buffers.collect();
        let simplex_in  = simplex::WithScratch::new(cfg_in .format, frame_count, buffers_in );
        let simplex_out = simplex::WithScratch::new(cfg_out.format, frame_count, buffers_out);

        callbacks.as_mut().prime(Arc::clone(self), data_cb, error_cb, simplex_in, simplex_out);
        
        Ok(callbacks)
    }
}

impl Hash for Session {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.driver.as_raw().as_raw().hash(state);
        self.init_success.hash(state);
        self.clsid_string.hash(state);
    }
}

#[derive(Debug)]
pub struct Stream {
    session    : Arc<Session>,
    frame_count: FrameCount,
    _callbacks : Pin<Box<Container>>
}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl StreamTrait for Stream {
    fn start(&self) -> CpalResult<()> {
        self.session
            .driver
            .start()
            .map_err(|error| create_report(&self.session.driver, error, "start"))
    }

    fn pause(&self) -> CpalResult<()> {
        self.session
            .driver
            .stop()
            .map_err(|error| create_report(&self.session.driver, error, "stop"))
    }

    fn stop(&self, _timeout: Option<Duration>) -> Result<(), Error> {
        self.pause()
    }

    fn now(&self) -> StreamInstant {
        self.session
            .driver
            .sample_position()
            .map_or(0, |pos| pos.time_stamp as u64)
            .pipe(StreamInstant::from_millis)
    }

    fn buffer_size(&self) -> CpalResult<FrameCount> {
        Ok(self.frame_count) // ASIO channels are always mono
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        _ = self.pause(); // might fail if the stream is already halted
        _ = self.session.driver.dispose_all_buffers(); // dunno in what kind of scenario this would fail
    }
}
