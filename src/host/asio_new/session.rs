use crate::ErrorKind::*;
use crate::host::com;
use crate::*;
use azo::dto::ChannelCounts;
use azo::{Driver, WinResult};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::ffi::CStr;
use std::fmt::Debug;
use std::hash::Hash;
use std::hash::Hasher;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::Duration;
use std::vec;
use tap::prelude::*;
use windows_core::GUID;

use super::callbacks::{Callbacks, Context};
use super::simplex::{In, Out, Simplex};
use super::utils::{CpalResult, DoubleBuffer, create_report, err};
use super::{SupportedConfigs, capabilities, simplex};

#[derive(Debug)]
pub struct Factory {
    com_worker: com::worker::Handle,
    cache: Mutex<HashMap<GUID, Weak<Session>>>,
}

impl Factory {
    pub fn new() -> Self {
        Self {
            com_worker: com::worker::Handle::new(),
            cache: Mutex::default(),
        }
    }

    pub fn get_session(&self, clsid: &GUID) -> WinResult<Arc<Session>> {
        let mut guard = self.cache.lock();

        if let Some(existing) = guard.get(clsid).and_then(Weak::upgrade) {
            return Ok(existing);
        }

        let new = Session
            ::new(*clsid, &self.com_worker)?
            .pipe(Arc::new);

        guard.insert(*clsid, Arc::downgrade(&new));

        Ok(new)
    }
}

#[derive(Debug)]
pub struct Session {
    state       : RwLock<State>,
    clsid_string: String,
    _com_worker : com::worker::Handle
}

#[derive(Debug)]
struct State {
    driver      : Driver,
    stage       : AsioStage,
}

impl Hash for Session {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.clsid_string.hash(state);
    }
}

impl Session {
    pub fn new(clsid: GUID, com_worker: &com::worker::Handle) -> WinResult<Self> {
        let driver = com_worker.create_driver(clsid)?;
        let init_success= driver.init(None);

        Self {
            state: State {
                driver,
                stage: if init_success { AsioStage::Initialized } else { AsioStage::Loaded },
            }.pipe(RwLock::new),
            clsid_string: format!("{clsid:?}"),
            _com_worker: com_worker.clone(), // hold on to this to keep the thread alive that initialized the COM apartment in which the driver was created
        }
        .pipe(Ok)
    }

    pub fn name(&self) -> String {
        self.state
            .read()
            .driver
            .name()
            .pipe_as_ref(CStr::to_string_lossy)
            .into_owned()
    }

    pub fn id(&self) -> CpalResult<DeviceId> {
        DeviceId::new(
            HostId::AsioNew,
            self.clsid_string.clone()
        )
        .pipe(Ok)
    }

    pub fn description(&self) -> CpalResult<DeviceDescription> {
        let state = self.state.read();

        let name_c = state.driver.name();
        let name = name_c.to_string_lossy();

        let direction = match capabilities::channel_counts(&state.driver)? {
            ChannelCounts { in_: 1.., out: 1.. } => DeviceDirection::Duplex,
            ChannelCounts { in_: 1.., out: 0   } => DeviceDirection::Input,
            ChannelCounts { in_: 0  , out: 1.. } => DeviceDirection::Output,
            _                                    => DeviceDirection::Unknown,
        };

        let mut extended = vec![format!("driver version: {}", state.driver.version())];

        if state.stage < AsioStage::Initialized {
            extended.push("ASIO driver failed to initialize".to_owned()); // ASIO drivers can often still do *something* when they fail to initialize
            extended.push(format!("last error: {}", state.driver.last_error().to_string_lossy()));
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
    pub fn supports_direction<const IN: bool, const OUT: bool>(&self) -> bool {
        let Ok(counts) = self.state.read().driver.channel_counts()
        else { return false; }; // can't do anything if it can't even count the channels

        if IN && counts.in_ == 0 {
            return false;
        }

        if OUT && counts.out == 0 {
            return false;
        }

        true
    }

    pub fn supported_configs<const INPUT: bool>(&self) -> CpalResult<SupportedConfigs> {
        let state = self.state.read();

        let ch_count             = capabilities::channel_count::<INPUT>(&state.driver)?;
        let (min_rate, max_rate) = capabilities::sample_rates(&state.driver)?;
        let buf_size             = capabilities::supported_buffer_size(&state.driver);
        let sample_formats       = capabilities::sample_formats::<INPUT>(&state.driver, ch_count)?;

        sample_formats
            .map(move |format| SupportedStreamConfigRange::new(ch_count as _, min_rate, max_rate, buf_size, format))
            .collect::<Vec<_>>()
            .into_iter()
            .pipe(Ok)
    }

    pub fn default_config<const INPUT: bool>(&self) -> CpalResult<SupportedStreamConfig> {
        self.supported_configs::<INPUT>()?
            .next()
            .ok_or(Error::with_message(UnsupportedOperation, "the device has no channels in this direction"))?
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
	
	pub fn build_stream(
        self       : &Arc<Self>,
        cfg_in     : simplex::Config,
        cfg_out    : simplex::Config,
        sample_rate: SampleRate,
        buffer_size: BufferSize,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!()
    ) -> CpalResult<super::Stream> {
        let mut state = self.state.write();

        if state.stage < AsioStage::Initialized {
            return err(DeviceNotAvailable, "ASIO driver failed to initialize");
        }
        if state.stage > AsioStage::Initialized {
            return err(UnsupportedOperation, "ASIO only supports 1 stream per device");
        }

        state.set_sample_rate(sample_rate)?;
        let frame_count = state.determine_buffer_size(buffer_size)?;
        let callbacks = state.prepare(Arc::clone(self), sample_rate, frame_count, cfg_in, cfg_out, data_cb, error_cb)?;

        state.stage = AsioStage::Prepared;

        super::Stream {
            session: Arc::clone(self),
            frame_count,
            _callbacks: callbacks // keep this alive until the stream is dropped
        }.pipe(Ok)
    }

    pub fn latencies(&self, sample_rate: SampleRate) -> CpalResult<[Duration; 2]> {
        self.state
            .read()
            .temporal_latencies(sample_rate)
    }

    pub fn start(&self) -> CpalResult<()> {
        let mut state = self.state.write();

        state.driver
            .start()
            .map_err(|error| create_report(&state.driver, error, stringify!(Driver::start)))
    }

    pub fn pause(&self) -> CpalResult<()> {
        let mut state = self.state.write();

        state.driver
            .stop()
            .map_err(|error| create_report(&state.driver, error, stringify!(Driver::stop)))
    }

    pub fn stop(&self) -> CpalResult<()> {
        todo!()
    }

    pub fn now(&self) -> StreamInstant {
        self.state
            .write()
            .driver
            .sample_position()
            .map_or(0, |pos| pos.time_stamp as u64) // `StreamTrait` requires this functio to be infallible
            .pipe(StreamInstant::from_millis)
    }

    pub fn reset(&self) {
        _ = self.pause(); // may fail if the stream is already halted

        let mut state = self.state.write();
        _ = state.driver.dispose_all_buffers(); // if something important goes wrong here, the driver will keep complaining in subsequent interactions
        state.stage = AsioStage::Initialized;
    }
}

impl State {
    fn set_sample_rate(&self, sample_rate: SampleRate) -> CpalResult<()> {
        self.driver
            .can_sample_rate(sample_rate as _)
            .map_err(|_| Error::with_message(InvalidInput, "sample rate not supported"))?;

        self.driver
            .set_sample_rate(sample_rate as _)
            .map_err(|asio_error| create_report(&self.driver, asio_error, stringify!(Driver::set_sample_rate)))?;

        Ok(())
    }

    fn determine_buffer_size(&self, requested: BufferSize) -> CpalResult<FrameCount> {
        match requested {
            BufferSize::Fixed(n) => n,
            BufferSize::Default  => capabilities::preferred_buffer_size(&self.driver)? as FrameCount,
        }
        .pipe(Ok)
    }

    fn temporal_latencies(&self, sample_rate: SampleRate) -> CpalResult<[Duration; 2]> {
        self.driver
            .latencies()
            .map_err(|error| create_report(&self.driver, error, stringify!(Driver::latencies)))?
            .pipe(|latencies| [latencies.in_, latencies.out])
            .map(|latency| latency as f64 / sample_rate as f64)
            .map(Duration::from_secs_f64)
            .pipe(Ok)
    }

    fn prepare(
        &self,
        session    : Arc<Session>,
        sample_rate: SampleRate,
        frame_count: FrameCount,
        cfg_in     : simplex::Config,
        cfg_out    : simplex::Config,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!()
    ) -> CpalResult<Pin<Box<Callbacks>>> {
        let channel_ids: Vec<_> = [cfg_in, cfg_out]
            .into_iter()
            .flat_map(|cfg| cfg.validate(&self.driver))
            .collect::<CpalResult<_>>()?;

        let [latency_in, latency_out] = self.temporal_latencies(sample_rate)?;

        // FIXME: consider using `Pin::defaul()` once MSRV has risen to 1.91+
        let mut callbacks = Callbacks::default().pipe(Box::pin);

        // SAFETY:
        // `Callbacks` is pinned, and kept alive until after the buffers are disposed (see `Drop` implementation of `Stream`)
        let mut double_buffers =
            unsafe { self.driver.create_buffers(channel_ids, frame_count as _, callbacks.pointers()) }
            .map_err(|error| create_report(&self.driver, error, stringify!(Driver::create_buffers)))?
            .map(DoubleBuffer);

        let buffers_in  = double_buffers.by_ref().take(cfg_in.channels as _).collect();
        let buffers_out = double_buffers.collect();

        let state = Context {
            data_cb    ,
            error_cb   ,
            sample_rate,
            session    ,
            simplex_in : Simplex::<In >::new(cfg_in .format, frame_count, buffers_in , latency_in ),
            simplex_out: Simplex::<Out>::new(cfg_out.format, frame_count, buffers_out, latency_out)
        };

        callbacks.as_mut().populate(state);

        Ok(callbacks)
    }
}

/// ASIO lifecycle stages (see ASIO specification section II.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AsioStage {
    Unloaded,
    Loaded,
    Initialized,
    Prepared,
    Running,
    Draining
}
