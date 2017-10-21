use std::marker::PhantomData;
use std::mem;
use std::os::raw::c_char;
use std::os::raw::c_int;
use std::os::raw::c_void;
use std::slice::from_raw_parts;
use stdweb;
use stdweb::unstable::TryInto;
use stdweb::web::TypedArray;

use CreationError;
use Format;
use FormatsEnumerationError;
use Sample;
use SupportedFormat;
use UnknownTypeBuffer;

extern {
    fn emscripten_set_main_loop_arg(_: extern fn(*mut c_void), _: *mut c_void, _: c_int, _: c_int);
    fn emscripten_run_script(script: *const c_char);
    fn emscripten_run_script_int(script: *const c_char) -> c_int;
}

// The emscripten backend works by having a global variable named `_cpal_audio_contexts`, which
// is an array of `AudioContext` objects. A voice ID corresponds to an entry in this array.
//
// Creating a voice creates a new `AudioContext`. Destroying a voice destroys it.

// TODO: handle latency better ; right now we just use setInterval with the amount of sound data
// that is in each buffer ; this is obviously bad, and also the schedule is too tight and there may
// be underflows

pub struct EventLoop;
impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        stdweb::initialize();
        EventLoop
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer)
    {
        unsafe {
            // The `run` function uses `emscripten_set_main_loop_arg` to invoke a Rust callback
            // repeatidely. The job of this callback is to fill the content of the audio buffers.

            // The first argument of the callback function (a `void*`) is a casted pointer to the
            // `callback` parameter that was passed to `run`.

            extern "C" fn callback_fn<F>(callback_ptr: *mut c_void)
                where F: FnMut(VoiceId, UnknownTypeBuffer)
            {
                unsafe {
                    let num_contexts = js!(
                        if (window._cpal_audio_contexts)
                            return window._cpal_audio_contexts.length;
                        else
                            return 0;
                    ).try_into().unwrap();

                    // TODO: this processes all the voices, even those from maybe other event loops
                    // this is not a problem yet, but may become one in the future?
                    for voice_id in 0 .. num_contexts {
                        let callback_ptr = &mut *(callback_ptr as *mut F);

                        let buffer = Buffer {
                            temporary_buffer: vec![0.0; 44100 * 2 / 3],
                            voice_id: voice_id,
                            marker: PhantomData,
                        };

                        callback_ptr(VoiceId(voice_id), ::UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) }));
                    }
                }
            }

            let callback_ptr = &mut callback as *mut F as *mut c_void;
            emscripten_set_main_loop_arg(callback_fn::<F>, callback_ptr, 3, 1);
            
            unreachable!()
        }
    }

    #[inline]
    pub fn build_voice(&self, _: &Endpoint, format: &Format)
                       -> Result<VoiceId, CreationError>
    {
        // TODO: find an empty element in the array first, instead of pushing at the end, in case
        // the user creates and destroys lots of voices?

        let num = js!(
            if (!window._cpal_audio_contexts)
                window._cpal_audio_contexts = new Array();
            window._cpal_audio_contexts.push(new AudioContext());
            return window._cpal_audio_contexts.length - 1;
        ).try_into().unwrap();

        Ok(VoiceId(num))
    }

    #[inline]
    pub fn destroy_voice(&self, voice_id: VoiceId) {
        let v = voice_id.0;
        js!(
            if (window._cpal_audio_contexts)
                window._cpal_audio_contexts[@{v}] = null;
        );
    }

    #[inline]
    pub fn play(&self, voice_id: VoiceId) {
        let v = voice_id.0;
        js!(
            if (window._cpal_audio_contexts)
                if (window._cpal_audio_contexts[@{v}])
                    window._cpal_audio_contexts[@{v}].resume();
        );
    }

    #[inline]
    pub fn pause(&self, voice_id: VoiceId) {
        let v = voice_id.0;
        js!(
            if (window._cpal_audio_contexts)
                if (window._cpal_audio_contexts[@{v}])
                    window._cpal_audio_contexts[@{v}].suspend();
        );
    }
}

// Index within the `_cpal_audio_contexts` global variable in Javascript.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceId(c_int);

// Detects whether the `AudioContext` global variable is available.
fn is_webaudio_available() -> bool {
    stdweb::initialize();

    js!(
        if (!AudioContext) { return false; } else { return true; }
    ).try_into().unwrap()
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
    pub fn supported_formats(
        &self)
        -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
        // TODO: right now cpal's API doesn't allow flexibility here
        //       "44100" and "2" (channels) have also been hard-coded in the rest of the code ; if
        //       this ever becomes more flexible, don't forget to change that
        Ok(vec![SupportedFormat {
            channels: vec![::ChannelPosition::BackLeft, ::ChannelPosition::BackRight],
            min_samples_rate: ::SamplesRate(44100),
            max_samples_rate: ::SamplesRate(44100),
            data_type: ::SampleFormat::F32,
        }].into_iter())
    }

    #[inline]
    pub fn name(&self) -> String {
        "Default endpoint".to_owned()
    }
}

pub type SupportedFormatsIterator = ::std::vec::IntoIter<SupportedFormat>;

pub struct Buffer<'a, T: 'a> where T: Sample {
    temporary_buffer: Vec<T>,
    voice_id: c_int,
    marker: PhantomData<&'a mut T>,
}

impl<'a, T> Buffer<'a, T> where T: Sample {
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
        unsafe {
            // TODO: directly use a TypedArray<f32> once this is supported by stdweb

            let typed_array = {
                let t_slice: &[T] = self.temporary_buffer.as_slice();
                let u8_slice: &[u8] = unsafe { from_raw_parts(t_slice.as_ptr() as *const _, t_slice.len() * mem::size_of::<T>()) };
                let typed_array: TypedArray<u8> = u8_slice.into();
                typed_array
            };

            let num_channels = 2u32;       // TODO: correct value
            debug_assert_eq!(self.temporary_buffer.len() % num_channels as usize, 0);

            let context = js!(
                if (!window._cpal_audio_contexts)
                    return;
                var context = window._cpal_audio_contexts[@{self.voice_id}];
                if (!context)
                    return;
                return context;
            ).into_reference();

            let context = match context {
                Some(c) => c,
                None => return,
            };

            js!(
                var src_buffer = new Float32Array(@{typed_array}.buffer);
                var context = @{context};
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
}
