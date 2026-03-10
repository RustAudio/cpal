use std::{thread::JoinHandle, time::Instant};

use crate::{
    host::fill_with_equilibrium, traits::StreamTrait, BackendSpecificError, InputCallbackInfo,
    OutputCallbackInfo, SampleFormat, StreamConfig, StreamError, StreamInstant,
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

use crate::Data;

#[derive(Debug, Clone, Copy)]
pub enum StreamCommand {
    Toggle(bool),
    Stop,
}

pub struct Stream {
    pub(crate) handle: Option<JoinHandle<()>>,
    pub(crate) controller: pw::channel::Sender<StreamCommand>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.controller.send(StreamCommand::Stop);
        let _ = self.handle.take().map(|handle| handle.join());
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), crate::PlayStreamError> {
        self.controller
            .send(StreamCommand::Toggle(true))
            .map_err(|_| crate::PlayStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "Cannot send message".to_owned(),
                },
            })?;
        Ok(())
    }
    fn pause(&self) -> Result<(), crate::PauseStreamError> {
        self.controller
            .send(StreamCommand::Toggle(false))
            .map_err(|_| crate::PauseStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "Cannot send message".to_owned(),
                },
            })?;
        Ok(())
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
    SampleFormat::I64,
    SampleFormat::U64,
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
    created_instance: Instant,
}
impl<D, E> UserData<D, E>
where
    E: FnMut(StreamError) + Send + 'static,
{
    fn state_changed(&mut self, new: StreamState) {
        match new {
            pipewire::stream::StreamState::Error(e) => {
                (self.error_callback)(StreamError::BackendSpecific {
                    err: BackendSpecificError { description: e },
                })
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
    if rc != 0 || t.now == 0 || t.rate.denom == 0 {
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
    E: FnMut(StreamError) + Send + 'static,
{
    fn publish_data_in(
        &mut self,
        stream: &pw::stream::Stream,
        frames: usize,
        data: &Data,
    ) -> Result<(), BackendSpecificError> {
        let (callback, capture) = match pw_stream_time(stream) {
            Some(PwTime { now_ns, delay_ns }) => (
                StreamInstant::from_nanos(now_ns),
                StreamInstant::from_nanos(now_ns - delay_ns),
            ),
            None => {
                let cb = stream_timestamp_fallback(self.created_instance)?;
                let pl = cb
                    .sub(frames_to_duration(frames, self.format.rate()))
                    .ok_or_else(|| BackendSpecificError {
                        description:
                            "`capture` occurs beyond representation supported by `StreamInstant`"
                                .to_string(),
                    })?;
                (cb, pl)
            }
        };
        let timestamp = crate::InputStreamTimestamp { callback, capture };
        let info = InputCallbackInfo { timestamp };
        (self.data_callback)(data, &info);
        Ok(())
    }
}
impl<D, E> UserData<D, E>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    fn publish_data_out(
        &mut self,
        stream: &pw::stream::Stream,
        frames: usize,
        data: &mut Data,
    ) -> Result<(), BackendSpecificError> {
        let (callback, playback) = match pw_stream_time(stream) {
            Some(PwTime { now_ns, delay_ns }) => (
                StreamInstant::from_nanos(now_ns),
                StreamInstant::from_nanos(now_ns + delay_ns),
            ),
            None => {
                let cb = stream_timestamp_fallback(self.created_instance)?;
                let pl = cb
                    .add(frames_to_duration(frames, self.format.rate()))
                    .ok_or_else(|| BackendSpecificError {
                        description:
                            "`playback` occurs beyond representation supported by `StreamInstant`"
                                .to_string(),
                    })?;
                (cb, pl)
            }
        };
        let timestamp = crate::OutputStreamTimestamp { callback, playback };
        let info = OutputCallbackInfo { timestamp };
        (self.data_callback)(data, &info);
        Ok(())
    }
}
pub struct StreamData<D, E> {
    pub mainloop: MainLoopRc,
    pub listener: StreamListener<UserData<D, E>>,
    pub stream: StreamRc,
    pub context: ContextRc,
}

// Use elapsed duration since stream creation as fallback when hardware timestamps are unavailable.
//
// This ensures positive values that are compatible with our `StreamInstant` representation.
#[inline]
fn stream_timestamp_fallback(
    creation: std::time::Instant,
) -> Result<StreamInstant, BackendSpecificError> {
    let now = std::time::Instant::now();
    let duration = now.duration_since(creation);
    StreamInstant::from_nanos_i128(duration.as_nanos() as i128).ok_or(BackendSpecificError {
        description: "stream duration has exceeded `StreamInstant` representation".to_string(),
    })
}

// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
#[inline]
fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

pub fn connect_output<D, E>(
    config: StreamConfig,
    properties: pw::properties::PropertiesBox,
    sample_format: SampleFormat,
    data_callback: D,
    error_callback: E,
) -> Result<StreamData<D, E>, pw::Error>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let data = UserData {
        data_callback,
        error_callback,
        sample_format,
        format: Default::default(),
        created_instance: Instant::now(),
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
                if current_channels != channels || rate != current_rate {
                    (user_data.error_callback)(StreamError::BackendSpecific {
                        err: BackendSpecificError {
                            description: format!("channels or rate is not fit, current channels: {current_channels}, current rate: {current_rate}"),
                        },
                    });
                    // if the channels and rate do not match, we stop the stream
                    if let Err(e) = stream.set_active(false) {
                        (user_data.error_callback)(StreamError::BackendSpecific {
                            err: BackendSpecificError {
                                description: format!("failed to stop the stream, reason: {e}"),
                            },
                        });
                    }
                }

            }
        })
        .state_changed(|_stream, user_data, _old, new| {
            user_data.state_changed(new);
        })
        .process(|stream, user_data| match stream.dequeue_buffer() {
            None => (user_data.error_callback)(StreamError::BufferUnderrun),
            Some(mut buffer) => {
                // Read the requested frame count before mutably borrowing datas_mut().
                let requested = buffer.requested() as usize;
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let buf_data = &mut datas[0];
                let n_channels = user_data.format.channels();

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

                // Pre-fill only the active region with silence before handing it to the callback.
                let active = &mut samples[..frames * stride];
                fill_with_equilibrium(active, user_data.sample_format);

                let data = active.as_mut_ptr() as *mut ();
                let mut data =
                    unsafe { Data::from_parts(data, n_samples, user_data.sample_format) };
                if let Err(err) = user_data.publish_data_out(stream, frames, &mut data) {
                    (user_data.error_callback)(StreamError::BackendSpecific { err });
                }
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

    // TODO: what about RT_PROCESS?
    /* Now connect this stream. We ask that our process function is
     * called in a realtime thread. */
    stream.connect(
        pw::spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
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
) -> Result<StreamData<D, E>, pw::Error>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    let data = UserData {
        data_callback,
        error_callback,
        sample_format,
        format: Default::default(),
        created_instance: Instant::now(),
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
                if current_channels != channels || rate != current_rate {
                    (user_data.error_callback)(StreamError::BackendSpecific {
                        err: BackendSpecificError {
                            description: format!("channels or rate is not fit, current channels: {current_channels}, current rate: {current_rate}"),
                        },
                    });
                    // if the channels and rate do not match, we stop the stream
                    if let Err(e) = stream.set_active(false) {
                        (user_data.error_callback)(StreamError::BackendSpecific {
                            err: BackendSpecificError {
                                description: format!("failed to stop the stream, reason: {e}"),
                            },
                        });
                    }
                }
            }
        })
        .state_changed(|_stream, user_data, _old, new| {
            user_data.state_changed(new);
        })
        .process(|stream, user_data| match stream.dequeue_buffer() {
            None => (user_data.error_callback)(StreamError::BufferUnderrun),
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let data = &mut datas[0];
                let n_channels = user_data.format.channels();
                let n_samples = data.chunk().size() / user_data.sample_format.sample_size() as u32;
                let frames = n_samples / n_channels;

                let Some(samples) = data.data() else {
                    return;
                };
                let data = samples.as_mut_ptr() as *mut ();
                let data =
                    unsafe { Data::from_parts(data, n_samples as usize, user_data.sample_format) };
                if let Err(err) = user_data.publish_data_in(stream, frames as usize, &data) {
                    (user_data.error_callback)(StreamError::BackendSpecific { err });
                }
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

    // TODO: what about RT_PROCESS?
    /* Now connect this stream. We ask that our process function is
     * called in a realtime thread. */
    stream.connect(
        pw::spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;

    Ok(StreamData {
        mainloop,
        listener,
        stream,
        context,
    })
}
