use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::Instant,
};

use pipewire::{
    self as pw,
    context::ContextRc,
    main_loop::MainLoopRc,
    spa::{
        param::{
            format::{MediaSubtype, MediaType},
            format_utils,
        },
        pod::Pod,
    },
    stream::{StreamListener, StreamRc, StreamState},
};

use crate::{
    host::{equilibrium::fill_equilibrium, frames_to_duration},
    traits::StreamTrait,
    Data, Error, ErrorKind, FrameCount, InputCallbackInfo, InputStreamTimestamp,
    OutputCallbackInfo, OutputStreamTimestamp, SampleFormat, StreamConfig, StreamInstant,
};

/// Counts the number of live [`PwInitGuard`] instances across all threads.
static PW_INIT_COUNT: Mutex<usize> = Mutex::new(0);

/// RAII guard that keeps the PipeWire library initialised for its lifetime.
pub(crate) struct PwInitGuard;

impl PwInitGuard {
    pub(crate) fn new() -> Self {
        let mut count = PW_INIT_COUNT.lock().unwrap_or_else(|e| e.into_inner());
        if *count == 0 {
            // pw::init() uses a OnceCell, preventing re-init after deinit.
            unsafe { pw::sys::pw_init(std::ptr::null_mut(), std::ptr::null_mut()) }
        }
        *count += 1;
        Self
    }
}

impl Drop for PwInitGuard {
    fn drop(&mut self) {
        let mut count = PW_INIT_COUNT.lock().unwrap_or_else(|e| e.into_inner());
        *count = count.saturating_sub(1);
        if *count == 0 {
            // Safety: the mutex ensures no other PwInitGuard exists at this
            // point. Every scope that creates PipeWire objects holds a guard
            // declared before those objects, so all PipeWire objects have
            // already been dropped before this decrement reached zero.
            unsafe { pw::deinit() }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StreamCommand {
    Toggle(bool),
    Stop,
}

pub struct Stream {
    pub(crate) handle: Option<JoinHandle<()>>,
    pub(crate) controller: pw::channel::Sender<StreamCommand>,
    pub(crate) last_quantum: Arc<AtomicU64>,
    pub(crate) start: Instant,
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.controller.send(StreamCommand::Stop);
        let _ = self.handle.take().map(|handle| handle.join());
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        self.controller
            .send(StreamCommand::Toggle(true))
            .map_err(|_| {
                Error::with_message(
                    ErrorKind::StreamInvalidated,
                    "stream command channel closed",
                )
            })?;
        Ok(())
    }
    fn pause(&self) -> Result<(), Error> {
        self.controller
            .send(StreamCommand::Toggle(false))
            .map_err(|_| {
                Error::with_message(
                    ErrorKind::StreamInvalidated,
                    "stream command channel closed",
                )
            })?;
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        monotonic_stream_instant().unwrap_or_else(|| stream_instant_from_start(self.start))
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.last_quantum.load(Ordering::Relaxed) as _)
    }
}

pub(crate) const SUPPORTED_FORMATS: &[SampleFormat] = &[
    SampleFormat::I8,
    SampleFormat::U8,
    SampleFormat::I16,
    SampleFormat::U16,
    SampleFormat::I24,
    SampleFormat::U24,
    SampleFormat::I32,
    SampleFormat::U32,
    // I64/U64 are excluded: libspa has no mapping for them yet.
    // SampleFormat::I64,
    // SampleFormat::U64,
    SampleFormat::F32,
    SampleFormat::F64,
];

impl From<SampleFormat> for pw::spa::param::audio::AudioFormat {
    fn from(value: SampleFormat) -> Self {
        match value {
            SampleFormat::I8 => Self::S8,
            SampleFormat::U8 => Self::U8,

            #[cfg(target_endian = "little")]
            SampleFormat::I16 => Self::S16LE,
            #[cfg(target_endian = "big")]
            SampleFormat::I16 => Self::S16BE,
            #[cfg(target_endian = "little")]
            SampleFormat::U16 => Self::U16LE,
            #[cfg(target_endian = "big")]
            SampleFormat::U16 => Self::U16BE,

            #[cfg(target_endian = "little")]
            SampleFormat::I24 => Self::S24LE,
            #[cfg(target_endian = "big")]
            SampleFormat::I24 => Self::S24BE,
            #[cfg(target_endian = "little")]
            SampleFormat::U24 => Self::U24LE,
            #[cfg(target_endian = "big")]
            SampleFormat::U24 => Self::U24BE,
            #[cfg(target_endian = "little")]
            SampleFormat::I32 => Self::S32LE,
            #[cfg(target_endian = "big")]
            SampleFormat::I32 => Self::S32BE,
            #[cfg(target_endian = "little")]
            SampleFormat::U32 => Self::U32LE,
            #[cfg(target_endian = "big")]
            SampleFormat::U32 => Self::U32BE,
            #[cfg(target_endian = "little")]
            SampleFormat::F32 => Self::F32LE,
            #[cfg(target_endian = "big")]
            SampleFormat::F32 => Self::F32BE,
            #[cfg(target_endian = "little")]
            SampleFormat::F64 => Self::F64LE,
            #[cfg(target_endian = "big")]
            SampleFormat::F64 => Self::F64BE,
            // NOTE: Seems PipeWire does support U64 and I64, but libspa doesn't yet.
            // TODO: Maybe add the support in the future
            _ => Self::Unknown,
        }
    }
}

pub struct UserData<D, E> {
    data_callback: D,
    error_callback: E,
    sample_format: SampleFormat,
    format: pw::spa::param::audio::AudioInfoRaw,
    last_quantum: Arc<AtomicU64>,
    start: Instant,
}
impl<D, E> UserData<D, E>
where
    E: FnMut(Error) + Send + 'static,
{
    fn state_changed(&mut self, new: StreamState) {
        match new {
            pipewire::stream::StreamState::Error(e) => {
                (self.error_callback)(Error::with_message(ErrorKind::StreamInvalidated, e))
            }
            // TODO: maybe we need to log information when every new state comes?
            pipewire::stream::StreamState::Paused => {}
            pipewire::stream::StreamState::Streaming => {}
            pipewire::stream::StreamState::Connecting => {}
            pipewire::stream::StreamState::Unconnected => {}
        }
    }
}

/// Hardware timestamp from a PipeWire graph cycle.
struct PwTime {
    /// CLOCK_MONOTONIC nanoseconds, stamped at the start of the graph cycle.
    now_ns: i64,
    /// Pipeline delay converted to nanoseconds.
    /// For output: how far ahead of the driver our next sample will be played.
    /// For input:  how long ago the data in the buffer was captured.
    delay_ns: i64,
}

/// Returns a hardware timestamp for the current graph cycle, or `None` if
/// the driver has not started yet or the rate is unavailable.
fn pw_stream_time(stream: &pw::stream::Stream) -> Option<PwTime> {
    let mut t: pw::sys::pw_time = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        pw::sys::pw_stream_get_time_n(
            stream.as_raw_ptr(),
            &mut t,
            std::mem::size_of::<pw::sys::pw_time>(),
        )
    };
    if rc != 0 || t.now <= 0 || t.rate.denom == 0 {
        return None;
    }
    debug_assert_eq!(t.rate.num, 1, "unexpected pw_time rate.num");
    let delay_ns = t.delay * 1_000_000_000i64 / t.rate.denom as i64;
    Some(PwTime {
        now_ns: t.now,
        delay_ns,
    })
}

impl<D, E> UserData<D, E>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(Error) + Send + 'static,
{
    fn publish_data_in(&mut self, stream: &pw::stream::Stream, frames: usize, data: &Data) {
        self.last_quantum.store(frames as u64, Ordering::Relaxed);
        let (callback, capture) = match pw_stream_time(stream) {
            Some(PwTime { now_ns, delay_ns }) => (
                StreamInstant::from_nanos(now_ns as u64),
                StreamInstant::from_nanos((now_ns - delay_ns.max(0)) as u64),
            ),
            None => {
                let cb = monotonic_stream_instant()
                    .unwrap_or_else(|| stream_instant_from_start(self.start));
                let capture = cb
                    .checked_sub(frames_to_duration(frames as FrameCount, self.format.rate()))
                    .unwrap_or(StreamInstant::ZERO);
                (cb, capture)
            }
        };
        let timestamp = InputStreamTimestamp { callback, capture };
        let info = InputCallbackInfo { timestamp };
        (self.data_callback)(data, &info);
    }
}
impl<D, E> UserData<D, E>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(Error) + Send + 'static,
{
    fn publish_data_out(&mut self, stream: &pw::stream::Stream, frames: usize, data: &mut Data) {
        self.last_quantum.store(frames as u64, Ordering::Relaxed);
        let (callback, playback) = match pw_stream_time(stream) {
            Some(PwTime { now_ns, delay_ns }) => (
                StreamInstant::from_nanos(now_ns as u64),
                StreamInstant::from_nanos((now_ns + delay_ns.max(0)) as u64),
            ),
            None => {
                let cb = monotonic_stream_instant()
                    .unwrap_or_else(|| stream_instant_from_start(self.start));
                let pl = cb + frames_to_duration(frames as FrameCount, self.format.rate());
                (cb, pl)
            }
        };
        let timestamp = OutputStreamTimestamp { callback, playback };
        let info = OutputCallbackInfo { timestamp };
        (self.data_callback)(data, &info);
    }
}
pub struct StreamData<D, E> {
    pub mainloop: MainLoopRc,
    pub listener: StreamListener<UserData<D, E>>,
    pub stream: StreamRc,
    pub context: ContextRc,
}

/// Fallback timestamp using elapsed time since stream creation.
fn stream_instant_from_start(start: Instant) -> StreamInstant {
    let elapsed = start.elapsed();
    StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos())
}

/// Read `clock_gettime` and return it as a [`StreamInstant`].
///
/// This is the same clock used by `pw_stream_get_time_n` (`pw_time.now`), so values
/// returned here are directly comparable with the `callback`/`capture`/`playback`
/// instants delivered to the data callback.
fn monotonic_stream_instant() -> Option<StreamInstant> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    if rc == 0 {
        Some(StreamInstant::new(ts.tv_sec as u64, ts.tv_nsec as u32))
    } else {
        None
    }
}

fn remote_props() -> Option<pw::properties::PropertiesBox> {
    let socket = super::utils::find_socket_path()?;
    let mut props = pw::properties::PropertiesBox::new();
    props.insert(*pw::keys::REMOTE_NAME, socket.to_string_lossy().as_ref());
    Some(props)
}

pub fn connect_output<D, E>(
    config: StreamConfig,
    properties: pw::properties::PropertiesBox,
    sample_format: SampleFormat,
    data_callback: D,
    error_callback: E,
    last_quantum: Arc<AtomicU64>,
    start: Instant,
) -> Result<StreamData<D, E>, pw::Error>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(Error) + Send + 'static,
{
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(remote_props())?;

    let data = UserData {
        data_callback,
        error_callback,
        sample_format,
        format: Default::default(),
        last_quantum,
        start,
    };
    let channels = config.channels as _;
    let rate = config.sample_rate as _;
    let stream = pw::stream::StreamRc::new(core, "cpal-playback", properties)?;
    let listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(move|stream, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) = match format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };

            // only accept raw audio
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }
            // call a helper function to parse the format for us.
            // When the format update, we check the format first, in case it does not fit what we
            // set
            if user_data.format.parse(param).is_ok() {
                let current_channels = user_data.format.channels();
                let current_rate = user_data.format.rate();
                let expected_fmt =
                    pw::spa::param::audio::AudioFormat::from(user_data.sample_format);
                let current_fmt = user_data.format.format();
                let mismatch = current_channels != channels
                    || current_rate != rate
                    || current_fmt != expected_fmt;
                if mismatch {
                    (user_data.error_callback)(Error::with_message(
                        ErrorKind::UnsupportedConfig,
                        format!("negotiated format mismatch: expected channels={channels} rate={rate} format={expected_fmt:?}, got channels={current_channels} rate={current_rate} format={current_fmt:?}"),
                    ));
                    // if the format does not match, we stop the stream
                    if let Err(e) = stream.set_active(false) {
                        (user_data.error_callback)(Error::with_message(
                            ErrorKind::StreamInvalidated,
                            format!("failed to stop the stream, reason: {e}"),
                        ));
                    }
                }

            }
        })
        .state_changed(|_stream, user_data, _old, new| {
            user_data.state_changed(new);
        })
        .process(|stream, user_data| {
            let n_channels = user_data.format.channels();
            if n_channels == 0 {
                return; // format not yet negotiated by param_changed
            }
            if let Some(mut buffer) = stream.dequeue_buffer() {
                // Read the requested frame count before mutably borrowing datas_mut().
                let requested = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let buf_data = &mut datas[0];

                let stride = user_data.sample_format.sample_size() * n_channels as usize;
                // frames = samples / channels or frames = data_len / stride
                // Honor the frame count PipeWire requests this cycle, capped by the
                // mapped buffer capacity to guard against any mismatch.
                let frames = requested.min(buf_data.as_raw().maxsize as usize / stride);
                let Some(samples) = buf_data.data() else {
                    return;
                };

                // samples = frames * channels or samples = data_len / sample_size
                let n_samples = frames * n_channels as usize;

                // Pre-fill only the active region with equilibrium before handing it to the
                // callback.
                let active = &mut samples[..frames * stride];
                fill_equilibrium(active, user_data.sample_format);

                let data = active.as_mut_ptr() as *mut ();
                let mut data =
                    unsafe { Data::from_parts(data, n_samples, user_data.sample_format) };
                user_data.publish_data_out(stream, frames, &mut data);
                let chunk = buf_data.chunk_mut();
                *chunk.offset_mut() = 0;
                *chunk.stride_mut() = stride as i32;
                *chunk.size_mut() = (frames * stride) as u32;
            }
        })
        .register()?;
    let mut audio_info = pw::spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(sample_format.into());
    audio_info.set_rate(rate);
    audio_info.set_channels(channels);

    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    // Connect the stream; RT_PROCESS schedules the process callback on
    // PipeWire's real-time driver thread.
    stream.connect(
        pw::spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    Ok(StreamData {
        mainloop,
        listener,
        stream,
        context,
    })
}
pub fn connect_input<D, E>(
    config: StreamConfig,
    properties: pw::properties::PropertiesBox,
    sample_format: SampleFormat,
    data_callback: D,
    error_callback: E,
    last_quantum: Arc<AtomicU64>,
    start: Instant,
) -> Result<StreamData<D, E>, pw::Error>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(Error) + Send + 'static,
{
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(remote_props())?;

    let data = UserData {
        data_callback,
        error_callback,
        sample_format,
        format: Default::default(),
        last_quantum,
        start,
    };

    let channels = config.channels as _;
    let rate = config.sample_rate as _;

    let stream = pw::stream::StreamRc::new(core, "cpal-capture", properties)?;
    let listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(move |stream, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) = match format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };

            // only accept raw audio
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }

            // call a helper function to parse the format for us.
            // When the format update, we check the format first, in case it does not fit what we
            // set
            if user_data.format.parse(param).is_ok() {
                let current_channels = user_data.format.channels();
                let current_rate = user_data.format.rate();
                let expected_fmt =
                    pw::spa::param::audio::AudioFormat::from(user_data.sample_format);
                let current_fmt = user_data.format.format();
                let mismatch = current_channels != channels
                    || current_rate != rate
                    || current_fmt != expected_fmt;
                if mismatch {
                    (user_data.error_callback)(Error::with_message(
                        ErrorKind::UnsupportedConfig,
                        format!("negotiated format mismatch: expected channels={channels} rate={rate} format={expected_fmt:?}, got channels={current_channels} rate={current_rate} format={current_fmt:?}"),
                    ));
                    // if the format does not match, we stop the stream
                    if let Err(e) = stream.set_active(false) {
                        (user_data.error_callback)(Error::with_message(
                            ErrorKind::StreamInvalidated,
                            format!("failed to stop the stream, reason: {e}"),
                        ));
                    }
                }
            }
        })
        .state_changed(|_stream, user_data, _old, new| {
            user_data.state_changed(new);
        })
        .process(|stream, user_data| {
            let n_channels = user_data.format.channels();
            if n_channels == 0 {
                return; // format not yet negotiated by param_changed
            }
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &mut datas[0];
                let n_samples = data.chunk().size() / user_data.sample_format.sample_size() as u32;
                let frames = n_samples / n_channels;

                let Some(samples) = data.data() else {
                    return;
                };
                let data = samples.as_mut_ptr() as *mut ();
                let data =
                    unsafe { Data::from_parts(data, n_samples as usize, user_data.sample_format) };
                user_data.publish_data_in(stream, frames as usize, &data);
            }
        })
        .register()?;
    let mut audio_info = pw::spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(sample_format.into());
    audio_info.set_rate(rate);
    audio_info.set_channels(channels);

    let obj = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    // Connect the stream; RT_PROCESS schedules the process callback on
    // PipeWire's real-time driver thread.
    stream.connect(
        pw::spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    Ok(StreamData {
        mainloop,
        listener,
        stream,
        context,
    })
}
