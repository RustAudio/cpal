use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::thread::{self, JoinHandle};

use crate::traits::HostTrait;
use crate::{
    Data, DevicesError, InputCallbackInfo, OutputCallbackInfo, OutputStreamTimestamp, SampleFormat,
    StreamConfig, StreamInstant, SupportedStreamConfigRange,
};

use pipewire::{properties::properties, spa};

mod device;
pub use self::device::Device;
pub use self::stream::Stream;
mod stream;

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type Devices = std::vec::IntoIter<Device>;

pub struct Host {
    tx: pipewire::channel::Sender<Message>,
    thread: Option<JoinHandle<()>>,

    devices_created: Vec<Device>,
}

impl Host {
    #[allow(dead_code)]
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        let (tx, rx) = pipewire::channel::channel::<Message>();
        let thread = thread::spawn(|| pw_thread(rx));
        Ok(Host {
            tx: tx.clone(),
            thread: Some(thread),
            devices_created: vec![Device { tx }],
        })
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Ok(self.devices_created.clone().into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.devices_created.first().cloned()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.devices_created.first().cloned()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.tx.send(Message::Destroy);
        self.thread.take().unwrap().join().unwrap();
    }
}

enum Message {
    Destroy,
    CreateOutputStream {
        id: usize,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>,
    },
    CreateInputStream {
        id: usize,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>,
    },
    DestroyStream {
        id: usize,
    },
}

fn pw_thread(rx: pipewire::channel::Receiver<Message>) {
    pipewire::init();

    let main_loop = pipewire::main_loop::MainLoop::new(None).unwrap();
    let context = pipewire::context::Context::new(&main_loop).unwrap();
    let core = context.connect(None).unwrap();

    let _receiver = rx.attach(main_loop.loop_(), {
        let main_loop = main_loop.clone();
        let core = core.clone();
        let streams = RefCell::new(BTreeMap::new());

        move |msg| match msg {
            Message::Destroy => {
                main_loop.quit();
            }
            Message::CreateOutputStream {
                id,
                config,
                sample_format,
                mut data_callback,
            } => {
                let stream = pipewire::stream::Stream::new(
                    &core,
                    "audio-src",
                    properties! {
                        *pipewire::keys::MEDIA_TYPE => "Audio",
                        *pipewire::keys::MEDIA_ROLE => "Music",
                        *pipewire::keys::MEDIA_CATEGORY => "Playback",
                        *pipewire::keys::AUDIO_CHANNELS => "2",
                    },
                )
                .unwrap();

                let _listener = stream
                    .add_local_listener::<()>()
                    .process(move |stream, _| {
                        let mut buffer = stream.dequeue_buffer().unwrap();
                        let datas = buffer.datas_mut();
                        let data = &mut datas[0];

                        let mut time: pipewire::sys::pw_time = unsafe { std::mem::zeroed() };
                        unsafe {
                            pipewire::sys::pw_stream_get_time_n(
                                stream.as_raw_ptr(),
                                &mut time,
                                std::mem::size_of_val(&time),
                            )
                        };

                        let stride = sample_format.sample_size() * config.channels as usize;
                        let sample_count = if let Some(data) = data.data() {
                            let sample_count = data.len() / stride;
                            let mut data = unsafe {
                                Data::from_parts(
                                    data.as_mut_ptr() as *mut (),
                                    data.len() / sample_format.sample_size(),
                                    sample_format,
                                )
                            };
                            data_callback(
                                &mut data,
                                &OutputCallbackInfo {
                                    timestamp: OutputStreamTimestamp {
                                        callback: StreamInstant::from_nanos(0),
                                        playback: StreamInstant::from_nanos(0),
                                    },
                                },
                            );
                            sample_count
                        } else {
                            0
                        };

                        let chunk = data.chunk_mut();
                        *chunk.offset_mut() = 0;
                        *chunk.stride_mut() = stride as _;
                        *chunk.size_mut() = (stride * sample_count) as _;
                    })
                    .register()
                    .unwrap();

                let audio_info_pod = audio_info(config, sample_format);
                stream
                    .connect(
                        spa::utils::Direction::Output,
                        None,
                        pipewire::stream::StreamFlags::AUTOCONNECT
                            | pipewire::stream::StreamFlags::MAP_BUFFERS
                            | pipewire::stream::StreamFlags::RT_PROCESS,
                        &mut [pipewire::spa::pod::Pod::from_bytes(&audio_info_pod).unwrap()],
                    )
                    .unwrap();

                streams.borrow_mut().insert(id, (stream, _listener));
            }
            Message::CreateInputStream {
                id: _,
                config: _,
                sample_format: _,
                data_callback: _,
            } => {
                todo!()
            }
            Message::DestroyStream { id } => {
                streams.borrow_mut().remove(&id);
            }
        }
    });

    main_loop.run();
}

fn audio_info(stream_config: StreamConfig, sample_format: SampleFormat) -> Vec<u8> {
    use pipewire::spa::{
        param::audio::{AudioFormat, AudioInfoRaw},
        pod::{serialize::PodSerializer, Object, Value},
        sys::{SPA_PARAM_EnumFormat, SPA_TYPE_OBJECT_Format},
    };

    let mut audio_info = AudioInfoRaw::new();
    audio_info.set_format(match sample_format {
        SampleFormat::I8 => AudioFormat::S8,
        SampleFormat::I16 => AudioFormat::S16LE,
        SampleFormat::I32 => AudioFormat::S32LE,
        SampleFormat::U8 => AudioFormat::U8,
        SampleFormat::U16 => AudioFormat::U16LE,
        SampleFormat::U32 => AudioFormat::U32LE,
        SampleFormat::F32 => AudioFormat::F32LE,
        SampleFormat::F64 => AudioFormat::F64LE,
        _ => todo!(),
    });
    audio_info.set_rate(stream_config.sample_rate.0);
    audio_info.set_channels(stream_config.channels as u32);

    PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &Value::Object(Object {
            type_: SPA_TYPE_OBJECT_Format,
            id: SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .unwrap()
    .0
    .into_inner()
}
