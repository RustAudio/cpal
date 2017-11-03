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
use std::slice;
use std::sync::{Arc, Mutex, Condvar};

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
    /// The active callback passed to `run`
    active_callback: Arc<Mutex<Option<&'static mut FnMut(VoiceId, UnknownTypeBuffer)>>>,
    voices: Mutex<Vec<Option<VoiceInner>>>,
    loop_cond: Arc<(Mutex<bool>, Condvar)>,
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
            active_callback: Arc::new(Mutex::new(None)),
            voices: Mutex::new(Vec::new()),
            loop_cond: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer)
    {
        let callback: &mut FnMut(VoiceId, UnknownTypeBuffer) = &mut callback;
        {
            let mut active_callback = self.active_callback.lock().unwrap();
            *active_callback = Some(unsafe { mem::transmute(callback) });
        }

        // Wait on a condvar to notify... which should never happen
        let &(ref lock, ref cvar) = &*self.loop_cond;
        let mut running = lock.lock().unwrap();
        *running = true;
        while *running {
            running = cvar.wait(running).unwrap();
        }

        unreachable!();
        // Note: if we ever change this API so that `run` can return, then it is critical that
        // we remove the callback from `active_callbacks`.
    }

    #[inline]
    pub fn build_voice(&self, _endpoint: &Endpoint, _format: &Format)
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
        let asbd = AudioStreamBasicDescription {
            mBitsPerChannel: 32,
            mBytesPerFrame: 8,
            mChannelsPerFrame: 2,
            mBytesPerPacket: 8,
            mFramesPerPacket: 1,
            mFormatFlags: (kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked) as u32,
            mFormatID: kAudioFormatLinearPCM,
            mSampleRate: 44100.0,
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
        let active_callback = self.active_callback.clone();
        audio_unit.set_render_callback(move |args: render_callback::Args<data::Raw>| unsafe {
            let AudioBuffer {
                mNumberChannels: _num_channels,
                mDataByteSize: data_byte_size,
                mData: data
            } = (*args.data.data).mBuffers[0];
            let data_slice = slice::from_raw_parts_mut(data as *mut f32, (data_byte_size / 4) as usize);

            // If `run()` is currently running, then a callback will be available.
            let mut callback_ref = active_callback.lock().unwrap();
            let callback = if let Some(ref mut cb) = *callback_ref {
                cb
            } else {
                for sample in data_slice.iter_mut() {
                    *sample = 0.0;
                }

                return Ok(());
            };

            let buffer = {
                Buffer {
                    buffer: data_slice
                }
            };

            callback(VoiceId(voice_id), UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) }));
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
