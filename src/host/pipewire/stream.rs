use std::{thread::JoinHandle, time::Duration};

use crate::{
    traits::StreamTrait, InputCallbackInfo, OutputCallbackInfo, SampleFormat, StreamConfig,
    StreamError,
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

#[allow(unused)]
pub struct Stream {
    pub(crate) handle: JoinHandle<()>,
    pub(crate) controller: pw::channel::Sender<bool>,
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), crate::PlayStreamError> {
        let _ = self.controller.send(true);
        Ok(())
    }
    fn pause(&self) -> Result<(), crate::PauseStreamError> {
        let _ = self.controller.send(false);
        Ok(())
    }
}

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
            SampleFormat::F32 => Self::F64BE,
            #[cfg(target_endian = "big")]
            SampleFormat::F32 => Self::F32BE,
            #[cfg(target_endian = "little")]
            SampleFormat::F64 => Self::F64LE,
            #[cfg(target_endian = "big")]
            SampleFormat::F64 => Self::F64BE,
            SampleFormat::I64 => Self::Unknown,
            SampleFormat::U64 => Self::Unknown,
        }
    }
}

pub struct UserData<D, E> {
    data_callback: D,
    error_callback: E,
    sample_format: SampleFormat,
    format: pw::spa::param::audio::AudioInfoRaw,
}
pub struct StreamData<D, E> {
    pub mainloop: MainLoopRc,
    pub listener: StreamListener<UserData<D, E>>,
    pub stream: StreamRc,
    pub context: ContextRc,
}
pub fn connect_output<D, E>(
    config: &StreamConfig,
    properties: pw::properties::PropertiesBox,
    sample_format: SampleFormat,
    data_callback: D,
    error_callback: E,
    _timeout: Option<Duration>,
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

                let Some(samples) = data.data() else {
                    return;
                };
                let data = samples.as_ptr() as *mut ();
                let _data =
                    unsafe { Data::from_parts(data, n_samples as usize, user_data.sample_format) };
                //(user_data.data_callback)(&data,crate::InputCallbackInfo::new(Ins) )
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
    _timeout: Option<Duration>,
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

                let Some(samples) = data.data() else {
                    return;
                };
                let data = samples.as_ptr() as *mut ();
                let _data =
                    unsafe { Data::from_parts(data, n_samples as usize, user_data.sample_format) };
                //(user_data.data_callback)(&data,crate::InputCallbackInfo::new(Ins) )
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
