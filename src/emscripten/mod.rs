use std::mem;
use std::os::raw::c_void;
use std::slice::from_raw_parts;
use std::sync::Mutex;
use stdweb;
use stdweb::Reference;
use stdweb::unstable::TryInto;
use stdweb::web::TypedArray;
use stdweb::web::set_timeout;

use CreationError;
use Format;
use FormatsEnumerationError;
use Sample;
use SupportedFormat;
use UnknownTypeBuffer;

// The emscripten backend works by having a global variable named `_cpal_audio_contexts`, which
// is an array of `AudioContext` objects. A voice ID corresponds to an entry in this array.
//
// Creating a voice creates a new `AudioContext`. Destroying a voice destroys it.

// TODO: handle latency better ; right now we just use setInterval with the amount of sound data
// that is in each buffer ; this is obviously bad, and also the schedule is too tight and there may
// be underflows

pub struct EventLoop {
    voices: Mutex<Vec<Option<Reference>>>,
}

impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        stdweb::initialize();

        EventLoop { voices: Mutex::new(Vec::new()) }
    }

    #[inline]
    pub fn run<F>(&self, callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer)
    {
        // The `run` function uses `set_timeout` to invoke a Rust callback repeatidely. The job
        // of this callback is to fill the content of the audio buffers.

        // The first argument of the callback function (a `void*`) is a casted pointer to `self`
        // and to the `callback` parameter that was passed to `run`.

        fn callback_fn<F>(user_data_ptr: *mut c_void)
            where F: FnMut(VoiceId, UnknownTypeBuffer)
        {
            unsafe {
                let user_data_ptr2 = user_data_ptr as *mut (&EventLoop, F);
                let user_data = &mut *user_data_ptr2;
                let user_cb = &mut user_data.1;

                let voices = user_data.0.voices.lock().unwrap().clone();
                for (voice_id, voice) in voices.iter().enumerate() {
                    let voice = match voice.as_ref() {
                        Some(v) => v,
                        None => continue,
                    };

                    let buffer = Buffer {
                        temporary_buffer: vec![0.0; 44100 * 2 / 3],
                        voice: &voice,
                    };

                    user_cb(VoiceId(voice_id),
                            ::UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) }));
                }

                set_timeout(|| callback_fn::<F>(user_data_ptr), 330);
            }
        }

        let mut user_data = (self, callback);
        let user_data_ptr = &mut user_data as *mut (_, _);

        set_timeout(|| callback_fn::<F>(user_data_ptr as *mut _), 10);

        stdweb::event_loop();
    }

    #[inline]
    pub fn build_voice(&self, _: &Endpoint, _format: &Format) -> Result<VoiceId, CreationError> {
        let voice = js!(return new AudioContext()).into_reference().unwrap();

        let mut voices = self.voices.lock().unwrap();
        let voice_id = if let Some(pos) = voices.iter().position(|v| v.is_none()) {
            voices[pos] = Some(voice);
            pos
        } else {
            let l = voices.len();
            voices.push(Some(voice));
            l
        };

        Ok(VoiceId(voice_id))
    }

    #[inline]
    pub fn destroy_voice(&self, voice_id: VoiceId) {
        self.voices.lock().unwrap()[voice_id.0] = None;
    }

    #[inline]
    pub fn play(&self, voice_id: VoiceId) {
        let voices = self.voices.lock().unwrap();
        let voice = voices
            .get(voice_id.0)
            .and_then(|v| v.as_ref())
            .expect("invalid voice ID");
        js!(@{voice}.resume());
    }

    #[inline]
    pub fn pause(&self, voice_id: VoiceId) {
        let voices = self.voices.lock().unwrap();
        let voice = voices
            .get(voice_id.0)
            .and_then(|v| v.as_ref())
            .expect("invalid voice ID");
        js!(@{voice}.suspend());
    }
}

// Index within the `voices` array of the events loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceId(usize);

// Detects whether the `AudioContext` global variable is available.
fn is_webaudio_available() -> bool {
    stdweb::initialize();

    js!(if (!AudioContext) {
            return false;
        } else {
            return true;
        }).try_into()
        .unwrap()
}

// Content is false if the iterator is empty.
pub struct EndpointsIterator(bool);
impl Default for EndpointsIterator {
    fn default() -> EndpointsIterator {
        // We produce an empty iterator if the WebAudio API isn't available.
        EndpointsIterator(is_webaudio_available())
    }
}
impl Iterator for EndpointsIterator {
    type Item = Endpoint;
    #[inline]
    fn next(&mut self) -> Option<Endpoint> {
        if self.0 {
            self.0 = false;
            Some(Endpoint)
        } else {
            None
        }
    }
}

#[inline]
pub fn default_endpoint() -> Option<Endpoint> {
    if is_webaudio_available() {
        Some(Endpoint)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    #[inline]
    pub fn supported_formats(&self) -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
        // TODO: right now cpal's API doesn't allow flexibility here
        //       "44100" and "2" (channels) have also been hard-coded in the rest of the code ; if
        //       this ever becomes more flexible, don't forget to change that
        Ok(
            vec![
                SupportedFormat {
                    channels: 2,
                    min_sample_rate: ::SampleRate(44100),
                    max_sample_rate: ::SampleRate(44100),
                    data_type: ::SampleFormat::F32,
                },
            ].into_iter(),
        )
    }

    #[inline]
    pub fn name(&self) -> String {
        "Default endpoint".to_owned()
    }
}

pub type SupportedFormatsIterator = ::std::vec::IntoIter<SupportedFormat>;

pub struct Buffer<'a, T: 'a>
    where T: Sample
{
    temporary_buffer: Vec<T>,
    voice: &'a Reference,
}

impl<'a, T> Buffer<'a, T>
    where T: Sample
{
    #[inline]
    pub fn buffer(&mut self) -> &mut [T] {
        &mut self.temporary_buffer
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.temporary_buffer.len()
    }

    #[inline]
    pub fn finish(self) {
        // TODO: directly use a TypedArray<f32> once this is supported by stdweb

        let typed_array = {
            let t_slice: &[T] = self.temporary_buffer.as_slice();
            let u8_slice: &[u8] = unsafe {
                from_raw_parts(t_slice.as_ptr() as *const _,
                               t_slice.len() * mem::size_of::<T>())
            };
            let typed_array: TypedArray<u8> = u8_slice.into();
            typed_array
        };

        let num_channels = 2u32; // TODO: correct value
        debug_assert_eq!(self.temporary_buffer.len() % num_channels as usize, 0);

        js!(
            var src_buffer = new Float32Array(@{typed_array}.buffer);
            var context = @{self.voice};
            var buf_len = @{self.temporary_buffer.len() as u32};
            var num_channels = @{num_channels};

            var buffer = context.createBuffer(num_channels, buf_len / num_channels, 44100);
            for (var channel = 0; channel < num_channels; ++channel) {
                var buffer_content = buffer.getChannelData(channel);
                for (var i = 0; i < buf_len / num_channels; ++i) {
                    buffer_content[i] = src_buffer[i * num_channels + channel];
                }
            }

            var node = context.createBufferSource();
            node.buffer = buffer;
            node.connect(context.destination);
            node.start();
        );
    }
}
