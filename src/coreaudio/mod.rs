extern crate coreaudio;
extern crate libc;

use ChannelPosition;
use CreationError;
use Format;
use FormatsEnumerationError;
use Sample;
use SampleFormat;
use SamplesRate;
use UnknownTypeBuffer;

use futures::Async;
use futures::Poll;
use futures::stream::Stream;
use futures::task;
use futures::task::Task;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use self::coreaudio::audio_unit::AudioUnit;
use self::coreaudio::audio_unit::render_callback::{self, data};

mod enumerate;

pub use self::enumerate::{EndpointsIterator, SupportedFormatsIterator, get_default_endpoint};

#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    pub fn get_supported_formats_list(
        &self)
        -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
        Ok(
            vec![
                Format {
                    channels: vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
                    samples_rate: SamplesRate(44100),
                    data_type: SampleFormat::F32,
                },
            ].into_iter(),
        )
    }

    pub fn get_name(&self) -> String {
        "Default AudioUnit Endpoint".to_string()
    }
}

pub struct EventLoop;
impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        EventLoop
    }
    #[inline]
    pub fn run(&self) {
        loop {
            // So the loop does not get optimised out in --release
            thread::sleep(Duration::new(1u64, 0u32));
        }
    }
}

pub struct Buffer<T> {
    args: render_callback::Args<data::NonInterleaved<T>>,
    buffer: Vec<T>,
}

impl<T> Buffer<T>
    where T: Sample
{
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

pub struct Voice {
    playing: bool,
    audio_unit: Arc<Mutex<AudioUnit>>,
}

#[allow(dead_code)] // the audio_unit will be dropped if we don't hold it.
pub struct SamplesStream {
    inner: Arc<Mutex<SamplesStreamInner>>,
    audio_unit: Arc<Mutex<AudioUnit>>,
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
            },
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
               -> Result<(Voice, SamplesStream), CreationError> {
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

        let au_type = if cfg!(target_os = "ios") {
            // The DefaultOutput unit isn't available in iOS unfortunately. RemoteIO is a sensible replacement.
            // See
            // https://developer.apple.com/library/content/documentation/MusicAudio/Conceptual/AudioUnitHostingGuide_iOS/UsingSpecificAudioUnits/UsingSpecificAudioUnits.html
            coreaudio::audio_unit::IOType::RemoteIO
        } else {
            coreaudio::audio_unit::IOType::DefaultOutput
        };
        let mut audio_unit = AudioUnit::new(au_type).map_err(convert_error)?;

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

            result.map_err(convert_error)?;
        }

        audio_unit.start().map_err(convert_error)?;

        let au_arc = Arc::new(Mutex::new(audio_unit));

        let samples_stream = SamplesStream {
            inner: inner,
            audio_unit: au_arc.clone(),
        };

        Ok((Voice {
                playing: true,
                audio_unit: au_arc.clone(),
            },
            samples_stream))
    }

    #[inline]
    pub fn play(&mut self) {
        if !self.playing {
            let mut unit = self.audio_unit.lock().unwrap();
            unit.start().unwrap();
            self.playing = true;
        }
    }

    #[inline]
    pub fn pause(&mut self) {
        if self.playing {
            let mut unit = self.audio_unit.lock().unwrap();
            unit.stop().unwrap();
            self.playing = false;
        }
    }
}
