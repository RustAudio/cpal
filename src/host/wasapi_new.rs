use self::device::default_id;
use self::device::Kind::Specific;
use self::processing::FnProcess;
use self::utils::*;
use crate::ErrorKind::{BackendError, ResourceExhausted, UnsupportedOperation};
use crate::traits::*;
use crate::*;
use std::borrow::Cow;
use std::{
    fmt::{self, Debug, Display},
    hash::Hash,
    time::Duration,
    sync::Arc,
    thread,
    vec,
};
use itertools::Itertools;
use parking_lot::Mutex;
use tap::Pipe;
use wasapi::{
    AudioClient, AudioClock, DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WaveFormat,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows_core::HSTRING;

#[cfg(feature = "realtime")]
use super::boost_current_thread_priority;

mod processing;
mod device;
mod utils;

type SupportedConfigs = vec::IntoIter<SupportedStreamConfigRange>;

pub struct Host;

impl Host {
    #[expect(
        clippy::unnecessary_wraps,
        reason = "required by the `impl_platform_host` macro"
    )]
    pub const fn new() -> Result<Self, CpalError> {
        Ok(Self)
    }

    fn default_device<const INPUT: bool>(&self) -> Option<Device> {
        if device::current_default_id::<INPUT>().ok()?.is_empty() {
            return None;
        }
        Device {
            kind: device::Kind::Default,
            direction: device::direction::<INPUT>()
        }
        .pipe(Some)
    }
}

impl HostTrait for Host {
    type Devices = vec::IntoIter<Device>;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, CpalError> {
        device::all()?
            .into_iter()
            .pipe(Ok)
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.default_device::<true>()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.default_device::<false>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    kind: device::Kind,
    direction: Direction,
}

impl Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.kind.hash(state);
        std::mem::discriminant(&self.direction).hash(state);
    }
}

impl Display for Device {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Specific(ref info) = self.kind
        else {
            return Display::fmt("Default", formatter);
        };

        if let Ok(name) = info.Name() {
            Display::fmt(&name, formatter)
        } else {
            Display::fmt("(device name lookup failed)", formatter)
        }
    }
}

impl Device {
    fn info(&self) -> Result<Cow<'_, DeviceInformation>, WindowsError> {
        if let Specific(ref info) = self.kind {
            return Cow::Borrowed(info).pipe(Ok);
        }

        match self.direction {
            Direction::Render  => device::current_default_info::<true >(),
            Direction::Capture => device::current_default_info::<false>()
        }
        .map(Cow::Owned)
    }

    fn native_id(&self) -> Result<HSTRING, WindowsError> {
        match self.kind {
            device::Kind::Default            => default_id(self.direction),
            device::Kind::Specific(ref info) => info.Id()
        }
    }

    fn activate(&self, timeout: Option<Duration>) -> Result<AudioClient, WindowsError> {
        self.native_id()
            .and_then(|id| activate_audio_interface(&id, timeout))
            .map(|client| AudioClient::from_iaudioclient(client, self.direction))
    }

    fn validate_direction<const INPUT: bool>(&self) -> Result<(), CpalError> {
        if self.direction == Direction::Capture && !INPUT {
            return CpalError::with_message(UnsupportedOperation, "cannot render on a capture device").pipe(Err);
        }

        Ok(())
    }

    fn default_config<const INPUT: bool>(&self) -> Result<SupportedStreamConfig, CpalError> {
        self.validate_direction::<INPUT>()?;

        self.activate(None)?
            .get_mixformat()?
            .try_into()
    }

    fn supported_configs<const INPUT: bool>(&self) -> Result<SupportedConfigs, CpalError> {
        use SampleFormat::*;

        self.validate_direction::<INPUT>()?;
        let client = self.activate(None)?;

        self.info()?
            .Id()?
            .to_string_lossy()
            .split('#')
            .nth(2)
            .ok_or(CpalError::with_message(BackendError, "malformed device id encountered"))?
            .pipe(|id| DeviceEnumerator::new()?.get_device(id))?
            .get_data_ranges()?
            .into_iter()
            .flat_map(move |range|
                COMMON_SAMPLE_RATES
                .iter()
                .copied()
                .skip_while(move |rate| *rate < range.min_samplerate)
                .take_while(move |rate| *rate <= range.max_samplerate)
                .flat_map(move |sample_rate|
                    (1..=range.max_channels)
                    .flat_map(move |channel_count|
                        [I8, I16, I24, I32, F32]
                        .into_iter()
                        .filter(move |format|
                            range
                            .sample_type()
                            .is_none_or(|ty|
                                (ty == SampleType::Int)
                                    == format.is_int()
                            )
                        )
                        .map(move |format| (format, format.sample_size() * 8))
                        .skip_while(move |(_format, store_bits)| range.min_bits_per_sample >  *store_bits as u32)
                        .take_while(move |(_format, store_bits)| range.max_bits_per_sample >= *store_bits as u32)
                        .map(move |(format, store_bits)| WaveFormat::new(
                            store_bits,
                            format.bits_per_sample() as _,
                            SampleType::Int,
                            sample_rate as _,
                            channel_count as _,
                            None
                        ))
                    )
                )
            )
            .unique() // DataRanges can overlap
            .filter_map(|waveformat|
                client
                .is_supported(&waveformat, ShareMode::Shared)
                .map(|ret| ret.unwrap_or(waveformat))
                .ok()
            )
            .unique() // `.is_supported()` might have reported the same "closest match" multiple times
            .filter_map(|waveformat| SupportedStreamConfig::try_from(waveformat).ok()) // this will most likely never fail anyway
            .map(|config| {
                let format = config.sample_format();
                let channels = config.channels();
                let rate = config.sample_rate();

                ((format, channels), rate)
            })
            .into_group_map()
            .into_iter()
            .map(|((sample_format, channels), rates)|
                SupportedStreamConfigRange {
                    channels,
                    min_sample_rate: rates.iter().copied().min().unwrap(),
                    max_sample_rate: rates.iter().copied().max().unwrap(),
                    buffer_size: SupportedBufferSize::default(),
                    sample_format,
                }
            )
            .collect::<Vec<_>>()
            .into_iter()
            .pipe(Ok)
    }

    #[expect(clippy::too_many_arguments, reason = "3 of these only exist due to certain language features not yet being stable (see comments)")]
    fn build_stream_inner<Processor, DataCallback, ErrorCallback>(
        &self,
        config          : StreamConfig,
        sample_format   : SampleFormat,
        mut data_cb     : DataCallback,
        mut error_cb    : ErrorCallback,
        timeout         : Option<Duration>,
        direction       : Direction, // const generics when?
        fn_new_processor: fn(&AudioClient) -> Result<Processor, WasapiRsError>, // unique function ZSTs when?
        fn_process      : FnProcess<Processor, DataCallback> // unique function ZSTs when?
    ) -> Result<Stream, CpalError>
    where
        Processor    : 'static,
        DataCallback : Send + 'static,
        ErrorCallback: FnMut(Error) + Send + 'static
    {
        let mut audio_client = self.activate(timeout)?;

        let waveformat = create_waveformat(config, sample_format)?;
        let period_frames = match config.buffer_size {
            BufferSize::Fixed(n) => n,
            BufferSize::Default => audio_client.get_shared_mode_engine_period(&waveformat)?.default
        };

        audio_client.initialize_client(
            &waveformat,
            direction,
            StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 },
            None
        )?;

        let processor    = audio_client.pipe_ref(fn_new_processor)?.pipe(SendSync);
        let audio_client = audio_client.pipe(SendSync).pipe(Arc::new);
        let audio_clock  = audio_client.get_audioclock()?.pipe(SendSync).pipe(Arc::new);
        let buffer_event = audio_client.set_get_eventhandle()?.pipe(SendSync);
        let total_frames = audio_client.get_buffer_size()?;
        let mut scratch  = ScratchBuffer::new(total_frames as usize * config.channels as usize, sample_format);
        let stream_state = WaitableStreamState::new()?.pipe(Arc::new);

        let audio_client2 = Arc::clone(&audio_client);
        let stream_state2 = Arc::clone(&stream_state);

        let dir = match direction {
            Direction::Capture => "in",
            Direction::Render => "out",
        };

        thread::Builder::new()
            .name(format!("cpal_wasapi_{dir}"))
            .spawn(move || {
                #[cfg(feature = "realtime")]
                let _rt_guard = boost_current_thread_priority(period_frames, config.sample_rate)
                    .map_err(&mut error_cb);
                
                _ = processing::run_loop(
                    &buffer_event,
                    &audio_client2,
                    &processor,
                    &stream_state2,
                    &mut scratch,
                    &mut data_cb,
                    fn_process,
                    sample_format,
                    config.channels as _,
                )
                .map_err(error_cb);
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to create WASAPI {dir}put worker thread: {e}"),
                )
            })?;

        StreamInner {
            total_frames,
            period_frames,
            audio_client,
            audio_clock,
            stream_state,
            last_hns: u64::MIN,
        }
        .pipe(Stream::from)
        .pipe(Ok)
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedConfigs;
    type SupportedOutputConfigs = SupportedConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, CpalError> {
        let info = self.info()?;
        let name = info.Name()?.to_string_lossy();

        let reader = device::PropertyReader(info.Properties()?);
        let (device_type, interface_type) = reader.types()?;

        let mut builder = DeviceDescriptionBuilder
            ::new(name.clone())
            .driver(name)
            .direction(self.direction.into())
            .device_type(device_type)
            .interface_type(interface_type);

        if let Some(physical_location) = reader.physical_location()? {
            builder = builder.address(physical_location);
        }

        Ok(builder.build())
    }

    fn id(&self) -> Result<DeviceId, CpalError> {
        self.info()?
            .Id()?
            .to_string_lossy()
            .into_boxed_str()
            .pipe(|id| DeviceId(HostId::WasapiNew, id))
            .pipe(Ok)
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, CpalError> {
        self.supported_configs::<true>()
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, CpalError> {
        self.supported_configs::<false>()
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, CpalError> {
        self.default_config::<true>()
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, CpalError> {
        self.default_config::<false>()
    }

    fn build_input_stream_raw<DataCallback, ErrorCallback>(
        &self,
        config  : StreamConfig,
        format  : SampleFormat,
        data_cb : DataCallback,
        error_cb: ErrorCallback,
        timeout : Option<Duration>,
    ) -> Result<Self::Stream, CpalError>
    where
        DataCallback : FnMut(&Data, &CallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(CpalError) + Send + 'static
    {
        self.build_stream_inner(
            config,
            format,
            data_cb,
            error_cb,
            timeout,
            Direction::Capture,
            AudioClient::get_audiocaptureclient,
            processing::capture
        )
    }

    fn build_output_stream_raw<DataCallback, ErrorCallback>(
        &self,
        config: StreamConfig,
        format: SampleFormat,
        data_cb: DataCallback,
        error_cb: ErrorCallback,
        timeout : Option<Duration>,
    ) -> Result<Self::Stream, CpalError>
    where
        DataCallback: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(CpalError) + Send + 'static,
    {
        self.build_stream_inner(
            config,
            format,
            data_cb,
            error_cb,
            timeout,
            Direction::Render,
            AudioClient::get_audiorenderclient,
            processing::render
        )
    }
}

#[derive(Debug)]
pub struct Stream(Mutex<StreamInner>);

impl From<StreamInner> for Stream {
    fn from(value: StreamInner) -> Self {
        value
            .pipe(Mutex::new)
            .pipe(Self)
    }
}
impl StreamTrait for Stream {
    fn start(&self) -> Result<(), CpalError> {
        self.0.lock().start()
    }

    fn pause(&self) -> Result<(), CpalError> {
        self.0.lock().pause()
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), CpalError> {
        self.0.lock().stop(timeout)
    }

    fn buffer_size(&self) -> Result<FrameCount, CpalError> {
        self.0.lock().buffer_size()
    }

    fn now(&self) -> StreamInstant {
        self.0.lock().now()
    }
}

#[derive(Debug)]
struct StreamInner {
    total_frames : u32,
    period_frames: u32,
    audio_client : Arc<SendSync<AudioClient>>,
    audio_clock  : Arc<SendSync<AudioClock>>,
    stream_state : Arc<WaitableStreamState>,
    last_hns     : u64
}

unsafe impl Send for StreamInner {}

impl StreamInner {
    pub fn start(&mut self) -> Result<(), CpalError> {
        if self.stream_state.get() == StreamState::Halted {
            self.audio_client.start_stream()?;
        }

        self.stream_state.set(StreamState::Running, true)?;

        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), CpalError> {
        if self.stream_state.get() != StreamState::Halted {
            self.audio_client.stop_stream()?;
        }

        self.stream_state.set(StreamState::Halted, true)?;

        Ok(())
    }

    pub fn stop(&mut self, timeout: Option<Duration>) -> Result<(), CpalError> {
        if self.stream_state.get() == StreamState::Halted {
            return Ok(());
        }

        self.stream_state.set(StreamState::Draining, false)?;

        let wait_success = self
            .stream_state
            .event()
            .wait_single(timeout)?;

        self.audio_client.stop_stream()?;
        self.audio_client.reset_stream()?;

        // FIXME: use `bool::ok_or()` once MSRV has risen to 1.98
        if !wait_success {
            return Err(CpalError::with_message(ResourceExhausted, "timeout expired"))
        }

        Ok(())
    }

    pub fn buffer_size(&self) -> Result<FrameCount, CpalError> {
        self.audio_client
            .get_current_padding()?
            .pipe(|padding| self.total_frames - padding + self.period_frames)
            .pipe(Ok)
    }

    pub fn now(&mut self) -> StreamInstant {
        self.last_hns = self
            .audio_clock
            .get_position()
            .map_or(u64::MIN, |(_pos, hns)| hns)
            .max(self.last_hns);

        StreamInstant::from_nanos(self.last_hns * 100)
    }
}

impl Drop for StreamInner {
    fn drop(&mut self) {
        _ = self.stream_state.set(StreamState::Exit, false);
    }
}
