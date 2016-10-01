extern crate coreaudio;
extern crate libc;

use CreationError;
use Format;
use FormatsEnumerationError;
use Sample;
use SampleFormat;
use SamplesRate;
use ChannelPosition;
use UnknownTypeBuffer;

use futures::Poll;
use futures::Async;
use futures::task::Task;
use futures::task;
use futures::stream::Stream;
use std::sync::{Arc, Mutex};

use self::coreaudio::audio_unit::AudioUnit;
use self::coreaudio::audio_unit::render_callback::{self, data};

mod enumerate;

pub use self::enumerate::{EndpointsIterator,
                          SupportedFormatsIterator,
                          get_default_endpoint};

#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        Ok(vec!(Format {
            channels: vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
            samples_rate: SamplesRate(44100),
            data_type: SampleFormat::F32
        }).into_iter())
    }

    pub fn get_name(&self) -> String {
        "Default AudioUnit Endpoint".to_string()
    }
}

pub struct EventLoop;
impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop { EventLoop }
    #[inline]
    pub fn run(&self) { loop {} }
}

pub struct Buffer<T> {
    args: render_callback::Args<data::NonInterleaved<T>>,
    buffer: Vec<T>,
}

impl<T> Buffer<T> where T: Sample {
    #[inline]
    pub fn get_buffer(&mut self) -> &mut [T] {
        &mut self.buffer[..]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn finish(self) {
        // TODO: At the moment this assumes the Vec<T> is a Vec<f32>.
        // Need to add T: Sample and use Sample::to_vec_f32.
        let Buffer { mut args, buffer } = self;

        let num_channels = args.data.channels().count();
        for (i, frame) in buffer.chunks(num_channels).enumerate() {
            for (channel, sample) in args.data.channels_mut().zip(frame.iter()) {
                channel[i] = *sample;
            }
        }
    }
}

type NumChannels = usize;
type NumFrames = usize;

pub struct Voice;

#[allow(dead_code)] // the audio_unit will be dropped if we don't hold it.
pub struct SamplesStream {
    audio_unit: AudioUnit,
    inner: Arc<Mutex<SamplesStreamInner>>,
}


struct SamplesStreamInner {
    scheduled_task: Option<Task>,
    current_callback: Option<render_callback::Args<data::NonInterleaved<f32>>>,
}

impl Stream for SamplesStream {
    type Item = UnknownTypeBuffer;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut inner = self.inner.lock().unwrap();

        // There are two possibilites: either we're answering a callback of coreaudio and we return
        // a buffer, or we're not answering a callback and we return that we're not ready.

        let current_callback = match inner.current_callback.take() {
            Some(c) => c,
            None => {
                inner.scheduled_task = Some(task::park());
                return Ok(Async::NotReady);
            }
        };

        let buffer_len = current_callback.num_frames * current_callback.data.channels().count();

        let buffer = Buffer {
            args: current_callback,
            buffer: vec![0.0; buffer_len],
        };

        Ok(Async::Ready(Some(UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) }))))
    }
}

impl Voice {
    pub fn new(_: &Endpoint, _: &Format, _: &EventLoop)
               -> Result<(Voice, SamplesStream), CreationError>
    {
        let inner = Arc::new(Mutex::new(SamplesStreamInner {
            scheduled_task: None,
            current_callback: None,
        }));

        fn convert_error(err: coreaudio::Error) -> CreationError {
            match err {
                coreaudio::Error::RenderCallbackBufferFormatDoesNotMatchAudioUnitStreamFormat |
                coreaudio::Error::NoKnownSubtype |
                coreaudio::Error::AudioUnit(coreaudio::error::AudioUnitError::FormatNotSupported) |
                coreaudio::Error::AudioCodec(_) |
                coreaudio::Error::AudioFormat(_) => CreationError::FormatNotSupported,
                _ => CreationError::DeviceNotAvailable,
            }
        }

        let au_type = coreaudio::audio_unit::IOType::DefaultOutput;
        let mut audio_unit = try!(AudioUnit::new(au_type).map_err(convert_error));

        // TODO: iOS uses integer and fixed-point data

        {
            let inner = inner.clone();
            let result = audio_unit.set_render_callback(move |args| {
                // This callback is entered whenever the coreaudio engine needs to be fed data.

                // Store the callback argument in the `SamplesStreamInner` and return the task
                // that we're supposed to notify.
                let scheduled = {
                    let mut inner = inner.lock().unwrap();

                    assert!(inner.current_callback.is_none());
                    inner.current_callback = Some(args);

                    inner.scheduled_task.take()
                };

                // It is important that `inner` is unlocked here.
                if let Some(scheduled) = scheduled {
                    // Calling `unpark()` should eventually call `poll()` on the `SamplesStream`,
                    // which will use the data we stored in `current_callback`.
                    scheduled.unpark();
                }

                // TODO: what should happen if the callback wasn't processed? in other word, what
                //       if the user didn't register any handler or did a stupid thing in the
                //       handler (like mem::forgetting the buffer)?

                Ok(())
            });

            try!(result.map_err(convert_error));
        }

        try!(audio_unit.start().map_err(convert_error));

        let samples_stream = SamplesStream {
            audio_unit: audio_unit,
            inner: inner,
        };

        Ok((Voice, samples_stream))
    }

    #[inline]
    pub fn play(&mut self) {
        // implicitly playing
    }

    #[inline]
    pub fn pause(&mut self) {
        unimplemented!()
    }
}
