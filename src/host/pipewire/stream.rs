use std::{thread::JoinHandle, time::Instant};

use crate::{
    traits::StreamTrait, BackendSpecificError, InputCallbackInfo, OutputCallbackInfo, SampleFormat,
    StreamConfig, StreamError,
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
    stream::{StreamListener, StreamRc},
};

use crate::Data;

#[derive(Debug, Clone, Copy)]
pub enum StreamCommand {
    Toggle(bool),
    Stop,
}

#[allow(unused)]
pub struct Stream {
    pub(crate) handle: JoinHandle<()>,
    pub(crate) controller: pw::channel::Sender<StreamCommand>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.controller.send(StreamCommand::Stop);
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), crate::PlayStreamError> {
        let _ = self.controller.send(StreamCommand::Toggle(true));
        Ok(())
    }
    fn pause(&self) -> Result<(), crate::PauseStreamError> {
        let _ = self.controller.send(StreamCommand::Toggle(false));
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
            // TODO: maybe we also need to support others
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
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    fn publish_data_in(&mut self, frames: usize, data: &Data) -> Result<(), BackendSpecificError> {
        let callback = stream_timestamp_fallback(self.created_instance)?;
        let delay_duration = frames_to_duration(frames, self.format.rate());
        let capture = callback
            .add(delay_duration)
            .ok_or_else(|| BackendSpecificError {
                description: "`playback` occurs beyond representation supported by `StreamInstant`"
                    .to_string(),
            })?;
        let timestamp = crate::InputStreamTimestamp { callback, capture };
        let info = crate::InputCallbackInfo { timestamp };
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
        frames: usize,
        data: &mut Data,
    ) -> Result<(), BackendSpecificError> {
        let callback = stream_timestamp_fallback(self.created_instance)?;
        let delay_duration = frames_to_duration(frames, self.format.rate());
        let playback = callback
            .add(delay_duration)
            .ok_or_else(|| BackendSpecificError {
                description: "`playback` occurs beyond representation supported by `StreamInstant`"
                    .to_string(),
            })?;
        let timestamp = crate::OutputStreamTimestamp { callback, playback };
        let info = crate::OutputCallbackInfo { timestamp };
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
) -> Result<crate::StreamInstant, BackendSpecificError> {
    let now = std::time::Instant::now();
    let duration = now.duration_since(creation);
    crate::StreamInstant::from_nanos_i128(duration.as_nanos() as i128).ok_or(BackendSpecificError {
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
    config: &StreamConfig,
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

    let stream = pw::stream::StreamRc::new(core, "cpal-playback", properties)?;
    let listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
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
            user_data
                .format
                .parse(param)
                .expect("Failed to parse param changed to AudioInfoRaw");
        })
        .process(|stream, user_data| match stream.dequeue_buffer() {
            None => (user_data.error_callback)(StreamError::BufferUnderrun),
            Some(mut buffer) => {
                let datas = buffer.datas_mut();
                if datas.is_empty() {
                    return;
                }
                let buf_data = &mut datas[0];
                let n_channels = user_data.format.channels();

                let Some(samples) = buf_data.data() else {
                    return;
                };
                let stride = user_data.sample_format.sample_size() * n_channels as usize;
                let frames = samples.len() / stride;

                let n_samples = samples.len() / user_data.sample_format.sample_size();

                let data = samples.as_ptr() as *mut ();
                let mut data =
                    unsafe { Data::from_parts(data, n_samples, user_data.sample_format) };
                if let Err(err) = user_data.publish_data_out(frames, &mut data) {
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
    audio_info.set_rate(config.sample_rate);
    audio_info.set_channels(config.channels as u32);

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

    /* Now connect this stream. We ask that our process function is
     * called in a realtime thread. */
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
    config: &StreamConfig,
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

    let stream = pw::stream::StreamRc::new(core, "cpal-capture", properties)?;
    let listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
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
            user_data
                .format
                .parse(param)
                .expect("Failed to parse param changed to AudioInfoRaw");
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
                let data = samples.as_ptr() as *mut ();
                let data =
                    unsafe { Data::from_parts(data, n_samples as usize, user_data.sample_format) };
                if let Err(err) = user_data.publish_data_in(frames as usize, &data) {
                    (user_data.error_callback)(StreamError::BackendSpecific { err });
                }
            }
        })
        .register()?;
    let mut audio_info = pw::spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(sample_format.into());
    audio_info.set_rate(config.sample_rate);
    audio_info.set_channels(config.channels as u32);

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

    /* Now connect this stream. We ask that our process function is
     * called in a realtime thread. */
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
