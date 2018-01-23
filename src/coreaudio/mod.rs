extern crate coreaudio;

use ChannelPosition;
use CreationError;
use Format;
use FormatsEnumerationError;
use Sample;
use SampleFormat;
use SamplesRate;
use SupportedFormat;
use UnknownTypeBuffer;

use std::mem;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::slice;

use self::coreaudio::audio_unit::{AudioUnit, Scope, Element};
use self::coreaudio::audio_unit::render_callback::{self, data};
use self::coreaudio::bindings::audio_unit::{
    AudioStreamBasicDescription,
    AudioBuffer,
    kAudioFormatLinearPCM,
    kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsPacked,
    kAudioUnitProperty_StreamFormat
};

mod enumerate;

pub use self::enumerate::{EndpointsIterator, SupportedFormatsIterator, default_endpoint};

#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    pub fn supported_formats(&self) -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
        Ok(
            vec![
                SupportedFormat {
                    channels: vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
                    min_samples_rate: SamplesRate(44100),
                    max_samples_rate: SamplesRate(44100),
                    data_type: SampleFormat::F32,
                },
            ].into_iter(),
        )
    }

    pub fn name(&self) -> String {
        "Default AudioUnit Endpoint".to_string()
    }
}

// The ID of a voice is its index within the `voices` array of the events loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceId(usize);

pub struct EventLoop {
    // This `Arc` is shared with all the callbacks of coreaudio.
    active_callbacks: Arc<ActiveCallbacks>,
    voices: Mutex<Vec<Option<VoiceInner>>>,
}

struct ActiveCallbacks {
    // Whenever the `run()` method is called with a callback, this callback is put in this list.
    callbacks: Mutex<Vec<&'static mut (FnMut(VoiceId, UnknownTypeBuffer) + Send)>>,
}

struct VoiceInner {
    playing: bool,
    audio_unit: AudioUnit,
}

// TODO need stronger error identification
impl From<coreaudio::Error> for CreationError {
    fn from(err: coreaudio::Error) -> CreationError {
        match err {
            coreaudio::Error::RenderCallbackBufferFormatDoesNotMatchAudioUnitStreamFormat |
            coreaudio::Error::NoKnownSubtype |
            coreaudio::Error::AudioUnit(coreaudio::error::AudioUnitError::FormatNotSupported) |
            coreaudio::Error::AudioCodec(_) |
            coreaudio::Error::AudioFormat(_) => CreationError::FormatNotSupported,
            _ => CreationError::DeviceNotAvailable,
        }
    }
}

impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        EventLoop {
            active_callbacks: Arc::new(ActiveCallbacks { callbacks: Mutex::new(Vec::new()) }),
            voices: Mutex::new(Vec::new()),
        }
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer) + Send
    {
        let callback: &mut (FnMut(VoiceId, UnknownTypeBuffer) + Send) = &mut callback;
        self.active_callbacks
            .callbacks
            .lock()
            .unwrap()
            .push(unsafe { mem::transmute(callback) });

        loop {
            // So the loop does not get optimised out in --release
            thread::sleep(Duration::new(1u64, 0u32));
        }

        // Note: if we ever change this API so that `run` can return, then it is critical that
        // we remove the callback from `active_callbacks`.
    }

    #[inline]
    pub fn build_voice(&self, _endpoint: &Endpoint, format: &Format)
                       -> Result<VoiceId, CreationError> {
        let mut audio_unit = {
            let au_type = if cfg!(target_os = "ios") {
                // The DefaultOutput unit isn't available in iOS unfortunately.
                // RemoteIO is a sensible replacement.
                // See https://goo.gl/CWwRTx
                coreaudio::audio_unit::IOType::RemoteIO
            } else {
                coreaudio::audio_unit::IOType::DefaultOutput
            };

            AudioUnit::new(au_type)?
        };

        // TODO: iOS uses integer and fixed-point data

        // Set the stream in interleaved mode.
        let n_channels = format.channels.len();
        let sample_rate = format.samples_rate.0;
        let bytes_per_channel = format.data_type.sample_size();
        let bits_per_channel = bytes_per_channel * 8;
        let bytes_per_frame = n_channels * bytes_per_channel;
        let frames_per_packet = 1;
        let bytes_per_packet = frames_per_packet * bytes_per_frame;
        let sample_format = format.data_type;
        let format_flags = match sample_format {
            SampleFormat::F32 => (kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked) as u32,
            _ => kAudioFormatFlagIsPacked as u32,
        };
        let asbd = AudioStreamBasicDescription {
            mBitsPerChannel: bits_per_channel,
            mBytesPerFrame: bytes_per_frame,
            mChannelsPerFrame: n_channels,
            mBytesPerPacket: bytes_per_packet,
            mFramesPerPacket: frames_per_packet,
            mFormatFlags: format_flags,
            mFormatID: kAudioFormatLinearPCM,
            mSampleRate: sample_rate as _,
            ..Default::default()
        };
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&asbd)
        )?;

        // Determine the future ID of the voice.
        let mut voices_lock = self.voices.lock().unwrap();
        let voice_id = voices_lock
            .iter()
            .position(|n| n.is_none())
            .unwrap_or(voices_lock.len());

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let active_callbacks = self.active_callbacks.clone();
        audio_unit.set_render_callback(move |args: render_callback::Args<data::Raw>| unsafe {
            // If `run()` is currently running, then a callback will be available from this list.
            // Otherwise, we just fill the buffer with zeroes and return.

            let AudioBuffer {
                mNumberChannels: _num_channels,
                mDataByteSize: data_byte_size,
                mData: data
            } = (*args.data.data).mBuffers[0];


            let mut callbacks = active_callbacks.callbacks.lock().unwrap();

            // A small macro to simplify handling the callback for different sample types.
            macro_rules! try_callback {
                ($SampleFormat:ident, $SampleType:ty, $equilibrium:expr) => {{
                    let data_len = (data_byte_size / bytes_per_channel) as usize;
                    let data_slice = slice::from_raw_parts_mut(data as *mut $SampleType, data_len);
                    let callback = match callbacks.get_mut(0) {
                        Some(cb) => cb,
                        None => {
                            for sample in data_slice.iter_mut() {
                                *sample = $equilibrium;
                            }
                            return Ok(());
                        }
                    };
                    let buffer = Buffer { buffer: data_slice };
                    let unknown_type_buffer = UnknownTypeBuffer::$SampleFormat(::Buffer { target: Some(buffer) });
                    callback(VoiceId(voice_id), unknown_type_buffer);
                }};
            }

            match sample_format {
                SampleFormat::F32 => try_callback!(F32, f32, 0.0),
                SampleFormat::I16 => try_callback!(I16, i16, 0),
                SampleFormat::U16 => try_callback!(U16, u16, ::std::u16::consts::MAX / 2),
            }

            Ok(())
        })?;

        // TODO: start playing now? is that consistent with the other backends?
        audio_unit.start()?;

        // Add the voice to the list of voices within `self`.
        {
            let inner = VoiceInner {
                playing: true,
                audio_unit: audio_unit,
            };

            if voice_id == voices_lock.len() {
                voices_lock.push(Some(inner));
            } else {
                voices_lock[voice_id] = Some(inner);
            }
        }

        Ok(VoiceId(voice_id))
    }

    pub fn destroy_voice(&self, voice_id: VoiceId) {
        let mut voices = self.voices.lock().unwrap();
        voices[voice_id.0] = None;
    }

    pub fn play(&self, voice: VoiceId) {
        let mut voices = self.voices.lock().unwrap();
        let voice = voices[voice.0].as_mut().unwrap();

        if !voice.playing {
            voice.audio_unit.start().unwrap();
            voice.playing = true;
        }
    }

    pub fn pause(&self, voice: VoiceId) {
        let mut voices = self.voices.lock().unwrap();
        let voice = voices[voice.0].as_mut().unwrap();

        if voice.playing {
            voice.audio_unit.stop().unwrap();
            voice.playing = false;
        }
    }
}

pub struct Buffer<'a, T: 'a> {
    buffer: &'a mut [T],
}

impl<'a, T> Buffer<'a, T>
    where T: Sample
{
    #[inline]
    pub fn buffer(&mut self) -> &mut [T] {
        &mut self.buffer
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn finish(self) {
        // Do nothing. We wrote directly to the buffer.
    }
}
