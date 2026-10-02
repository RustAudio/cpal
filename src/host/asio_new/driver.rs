use self::callbacks::Callbacks;
use super::SupportedConfigs;
use super::buffer;
use super::utils::{CpalResult, Decorate, err, sample_format_asio2cpal};
use crate::ErrorKind::*;
use crate::*;
use azo::WinResult;
use azo::driver::{Driver, Proxy};
use azo::dto::{ChannelCounts, ChannelId};
use azo::utils::Host as AzoHost;
use azo::windows_core::GUID;
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tap::Pipe;

mod callbacks;

/// Keeps track of all created [`Session`]s to prevent creating
/// multiple instances of the same driver in the same `AzoHost`
#[derive(Debug)]
pub struct Factory {
    azo_host: Arc<AzoHost>,
    cache: Mutex<HashMap<GUID, Weak<Session>>>,
}

impl Factory {
    pub fn new() -> Self {
        Self {
            azo_host: AzoHost::new(),
            cache: Mutex::default(),
        }
    }

    pub fn get_session(&self, clsid: &GUID) -> WinResult<Arc<Session>> {
        let mut guard = self.cache.lock();

        if let Some(existing) = guard.get(clsid).and_then(Weak::upgrade) {
            return Ok(existing);
        }

        let driver = self
            .azo_host
            .create_driver(*clsid)?
            .pipe(Handle);

        let new = Session
            ::new(driver, clsid)?
            .pipe(Arc::new);

        guard.insert(*clsid, Arc::downgrade(&new));

        Ok(new)
    }
}

/// A "cpal-ified" driver handle where relevant types (such as errors) are mapped to cpal's equivalents
#[derive(Debug)]
struct Handle(Proxy);

impl Handle {
    pub fn name(&self) -> String {
        self.0
            .name()
            .to_string_lossy()
            .into_owned()
    }

    pub fn channel_count<const INPUT: bool>(&self) -> CpalResult<i32> {
        self.channel_counts()
            .map(|counts|
                if INPUT { counts.in_ }
                else     { counts.out }
            )
    }

    pub fn channel_counts(&self) -> CpalResult<ChannelCounts> {
        self.0
            .channel_counts()
            .map_err(|error| Error::with_message(BackendError, format!("failed to retrieve channel coounts: {error}")))
    }

    pub fn sample_rates(&self) -> CpalResult<(SampleRate, SampleRate)> {
        let mut rates_iter = COMMON_SAMPLE_RATES
            .iter()
            .copied()
            .filter(|rate| self.0.can_sample_rate(*rate as _).is_ok());

        let min = rates_iter.next().ok_or(Error::with_message(DeviceNotAvailable, "no supported sample rate found"))?;
        let max = rates_iter.next_back().unwrap_or(min);

        Ok((min, max))
    }

    pub fn supported_buffer_size(&self) -> SupportedBufferSize {
        self.0
            .buffer_size()
            .map_or(SupportedBufferSize::Unknown, Into::into)
    }

    pub fn preferred_buffer_size(&self) -> CpalResult<i32> {
        let value = self
            .0
            .buffer_size()
            .decorate(&self.0, stringify!(Driver::buffer_size))?
            .preferred;

        if value.is_negative() {
            return err(BackendError, format!("ASIO driver reported invalid buffer size {value}"));
        }

        Ok(value)
    }

    pub fn sample_formats<const INPUT: bool>(&self, ch_count: i32) -> CpalResult<impl Iterator<Item = SampleFormat>> {
        (0..ch_count)
            .map(move |index|
                self.0
                    .channel_info(ChannelId { index, input: INPUT })
                    .map(|ch_info| ch_info.sample_type)
                    .decorate(&self.0, stringify!(Driver::channel_info))
            )
            .collect::<CpalResult<HashSet<_>>>()? // aggregates errors and deduplicates the sample types
            .into_iter()
            .filter_map(sample_format_asio2cpal)
            .pipe(Ok)
    }

    fn set_sample_rate(&self, sample_rate: SampleRate) -> CpalResult<()> {
        self.0
            .can_sample_rate(sample_rate as _)
            .map_err(|_| Error::with_message(InvalidInput, "sample rate not supported"))?;

        self.0
            .set_sample_rate(sample_rate as _)
            .decorate(&self.0, stringify!(Driver::set_sample_rate))?;

        Ok(())
    }

    fn choose_buffer_size(&self, requested: BufferSize) -> CpalResult<FrameCount> {
        match requested {
            BufferSize::Fixed(n) => n,
            BufferSize::Default  => self.preferred_buffer_size()? as FrameCount,
        }
        .pipe(Ok)
    }

    fn latencies(&self) -> CpalResult<[Duration; 2]> {
        let sample_rate = self
            .0
            .get_sample_rate()
            .decorate(&self.0, stringify!(Driver::get_sample_rate))?;

        self.0
            .latencies()
            .decorate(&self.0, stringify!(Driver::latencies))?
            .pipe(|latencies| [latencies.in_, latencies.out])
            .map(|latency| latency as f64 / sample_rate)
            .map(Duration::from_secs_f64)
            .pipe(Ok)
    }

    fn prepare(
        &self,
        session    : Arc<Session>,
        frame_count: FrameCount,
        cfg_in     : buffer::Config,
        cfg_out    : buffer::Config,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
    ) -> CpalResult<Pin<Box<Callbacks>>> {
        let channel_ids: Vec<_> = [cfg_in, cfg_out]
            .into_iter()
            .flat_map(|cfg| cfg.validate(&self.0))
            .collect::<CpalResult<_>>()?;

        let latencies = self.latencies()?;

        let mut callbacks = Callbacks::new(session, data_cb, error_cb, latencies);

        // SAFETY:
        // `Callbacks` is pinned, and kept alive until the buffers got disposed.
        // (see the `Drop` implementation of `Stream`)
        let mut buf_ptrs =
            unsafe { self.0.create_buffers(channel_ids, frame_count as _, callbacks.fn_pointers()) }
            .decorate(&self.0, stringify!(Driver::create_buffers))?;

        todo!("set buffers");

        Ok(callbacks)
    }

    fn dispose_buffers(&self) -> CpalResult<()> {
        self.0
            .dispose_buffers()
            .decorate(&self.0, stringify!(Driver::dispose_all_buffers))
    }
}

#[derive(Debug)]
pub struct Session {
    driver: Handle,
    stage: RwLock<Stage>,
    clsid_string: String,
}

impl Session {
	fn new(driver: Handle, clsid: &GUID) -> WinResult<Self> {
        let stage =
            if driver.0.init(None) { Stage::Initialized }
            else                   { Stage::Loaded };

        Self {
            driver,
            stage: stage.into(),
            clsid_string: format!("{clsid:?}"),
        }
        .pipe(Ok)
    }

    pub fn id(&self) -> DeviceId {
        DeviceId::new(HostId::AsioNew, &self.clsid_string)
    }

    pub fn display_name(&self) -> String {
        self.driver.name()
    }

    pub fn description(&self) -> CpalResult<DeviceDescription> {
        let stage = self.stage.read();

        let name = self.driver.name();
        let direction = self.driver.channel_counts()?.into();
        let mut extended = vec![format!("driver version: {}", self.driver.0.version())];

        if matches!(*stage, Stage::Loaded) {
            extended.push("ASIO driver failed to initialize".to_owned());
            extended.push(format!("last error: {}", self.driver.0.last_error().to_string_lossy()));
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
        let Ok(counts) = self.driver.channel_counts()
        else { return false; }; // cannot "support" anything if it can't even count the channels

        if IN && counts.in_ == 0 {
            return false;
        }

        if OUT && counts.out == 0 {
            return false;
        }

        true
    }

    pub fn supported_configs<const INPUT: bool>(&self) -> CpalResult<SupportedConfigs> {
        let ch_count             = self.driver.channel_count::<INPUT>()?;
        let (min_rate, max_rate) = self.driver.sample_rates()?;
        let buf_size             = self.driver.supported_buffer_size();
        let sample_formats       = self.driver.sample_formats::<INPUT>(ch_count)?;

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
                    range.sample_format(),
                )
            )
            .pipe(Ok)
    }

    #[expect(clippy::used_underscore_binding, reason = "semantically unused")]
    pub fn build_stream(
        self       : &Arc<Self>,
        cfg_in     : buffer::Config,
        cfg_out    : buffer::Config,
        sample_rate: SampleRate,
        buffer_size: BufferSize,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
    ) -> CpalResult<super::Stream> {
        let mut stage = self.stage.write();

        match *stage {
            Stage::Loaded          => return err(DeviceNotAvailable, "ASIO driver failed to initialize"),
            Stage::Initialized     => (), // this is the appropiate stage for calling this function
            Stage::Prepared { .. } => return err(UnsupportedOperation, "ASIO only supports 1 stream per device, and there already exists a stream for this device"),
        }

        self.driver.set_sample_rate(sample_rate)?;
        let frame_count = self.driver.choose_buffer_size(buffer_size)?;
        let _callbacks = self.driver.prepare(Arc::clone(self), frame_count, cfg_in, cfg_out, data_cb, error_cb)?;

        *stage = Stage::Prepared {
            running: false,
            _callbacks,
        };

        super::Stream {
            session: Arc::clone(self),
        }
        .pipe(Ok)
    }

    pub fn start(&self) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if *stage.running()? {
            return Ok(());
        }

        todo!();

        *stage.running()? = true;
        Ok(())
    }

    pub fn pause(&self) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if !*stage.running()? {
            return Ok(());
        }

        todo!();

        *stage.running()? = false;
        Ok(())
    }

    pub fn stop(&self, max_wait: Option<Duration>) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if !*stage.running()? {
            return Ok(());
        }

        todo!();

        *stage.running()? = false;
        Ok(())
    }

    pub fn now(&self) -> StreamInstant {
        todo!()
    }

    pub fn reset(&self) -> CpalResult<()> {
        let mut stage = self.stage.write();

        if *stage.running()? {
            todo!();
        }

        self.driver.dispose_buffers()?;

        *stage = Stage::Initialized;
        Ok(())
    }
}

/// ASIO lifecycle stage (see ASIO specification section II.2)
#[derive(Debug)]
pub enum Stage {
    Loaded,
    Initialized,
    Prepared {
        _callbacks: Pin<Box<Callbacks>>,
        running: bool,
    },
}

impl Stage {
    fn running(&mut self) -> CpalResult<&mut bool> {
        match self {
            Self::Prepared { running, .. } => Ok(running),
            _ => err(Other, "BUG! This branch should be unreachable"),
        }
    }
}
