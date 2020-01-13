use std::mem;
use std::os::raw::c_void;
use std::slice::from_raw_parts;
use stdweb;
use stdweb::Reference;
use stdweb::unstable::TryInto;
use stdweb::web::TypedArray;
use stdweb::web::set_timeout;

use BuildStreamError;
use DefaultFormatError;
use DeviceNameError;
use DevicesError;
use Format;
use PauseStreamError;
use PlayStreamError;
use SupportedFormatsError;
use StreamData;
use StreamError;
use SupportedFormat;
use UnknownTypeOutputBuffer;
use traits::{DeviceTrait, HostTrait, StreamTrait};

// The emscripten backend currently works by instantiating an `AudioContext` object per `Stream`.
// Creating a stream creates a new `AudioContext`. Destroying a stream destroys it. Creation of a
// `Host` instance initializes the `stdweb` context.

/// The default emscripten host type.
#[derive(Debug)]
pub struct Host;

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Stream {
    // A reference to an `AudioContext` object.
    audio_ctxt_ref: Reference,
}

// Index within the `streams` array of the events loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

pub type SupportedInputFormats = ::std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = ::std::vec::IntoIter<SupportedFormat>;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        stdweb::initialize();
        Ok(Host)
    }
}

impl Devices {
    fn new() -> Result<Self, DevicesError> {
        Ok(Self::default())
    }
}

impl Device {
    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("Default Device".to_owned())
    }

    #[inline]
    fn supported_input_formats(&self) -> Result<SupportedInputFormats, SupportedFormatsError> {
        unimplemented!();
    }

    #[inline]
    fn supported_output_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        // TODO: right now cpal's API doesn't allow flexibility here
        //       "44100" and "2" (channels) have also been hard-coded in the rest of the code ; if
        //       this ever becomes more flexible, don't forget to change that
        //       According to https://developer.mozilla.org/en-US/docs/Web/API/BaseAudioContext/createBuffer
        //       browsers must support 1 to 32 channels at leats and 8,000 Hz to 96,000 Hz.
        //
        //       UPDATE: We can do this now. Might be best to use `crate::COMMON_SAMPLE_RATES` and
        //       filter out those that lay outside the range specified above.
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

    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!();
    }

    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        // TODO: because it is hard coded, see supported_output_formats.
        Ok(
            Format {
                channels: 2,
                sample_rate: ::SampleRate(44100),
                data_type: ::SampleFormat::F32,
            },
        )
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume this host is always available on emscripten.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
}

impl DeviceTrait for Device {
    type SupportedInputFormats = SupportedInputFormats;
    type SupportedOutputFormats = SupportedOutputFormats;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    fn supported_input_formats(&self) -> Result<Self::SupportedInputFormats, SupportedFormatsError> {
        Device::supported_input_formats(self)
    }

    fn supported_output_formats(&self) -> Result<Self::SupportedOutputFormats, SupportedFormatsError> {
        Device::supported_output_formats(self)
    }

    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_input_format(self)
    }

    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_output_format(self)
    }

    fn build_input_stream<D, E>(
        &self,
        _format: &Format,
        _data_callback: D,
        _error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(StreamData) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        unimplemented!()
    }

    fn build_output_stream<D, E>(
        &self,
        _format: &Format,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(StreamData) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Create the stream.
        let audio_ctxt_ref = js!(return new AudioContext()).into_reference().unwrap();
        let stream = Stream { audio_ctxt_ref };

        // Specify the callback.
        let mut user_data = (self, data_callback, error_callback);
        let user_data_ptr = &mut user_data as *mut (_, _, _);

        // Use `set_timeout` to invoke a Rust callback repeatedly.
        //
        // The job of this callback is to fill the content of the audio buffers.
        //
        // See also: The call to `set_timeout` at the end of the `audio_callback_fn` which creates
        // the loop.
        set_timeout(|| audio_callback_fn::<D, E>(user_data_ptr as *mut c_void), 10);

        Ok(stream)
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let audio_ctxt = &self.audio_ctxt_ref;
        js!(@{audio_ctxt}.resume());
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        let audio_ctxt = &self.audio_ctxt_ref;
        js!(@{audio_ctxt}.suspend());
        Ok(())
    }
}

// The first argument of the callback function (a `void*`) is a casted pointer to `self`
// and to the `callback` parameter that was passed to `run`.
fn audio_callback_fn<D, E>(user_data_ptr: *mut c_void)
where
    D: FnMut(StreamData) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    unsafe {
        let user_data_ptr2 = user_data_ptr as *mut (&Stream, D, E);
        let user_data = &mut *user_data_ptr2;
        let (ref stream, ref mut data_cb, ref mut _err_cb) = user_data;
        let audio_ctxt = &stream.audio_ctxt_ref;

        // TODO: We should be re-using a buffer.
        let mut temporary_buffer = vec![0.0; 44100 * 2 / 3];

        {
            let buffer = UnknownTypeOutputBuffer::F32(::OutputBuffer { buffer: &mut temporary_buffer });
            let data = StreamData::Output { buffer: buffer };
            data_cb(data);
        }

        // TODO: directly use a TypedArray<f32> once this is supported by stdweb
        let typed_array = {
            let f32_slice = temporary_buffer.as_slice();
            let u8_slice: &[u8] = from_raw_parts(
                f32_slice.as_ptr() as *const _,
                f32_slice.len() * mem::size_of::<f32>(),
            );
            let typed_array: TypedArray<u8> = u8_slice.into();
            typed_array
        };

        let num_channels = 2u32; // TODO: correct value
        debug_assert_eq!(temporary_buffer.len() % num_channels as usize, 0);

        js!(
            var src_buffer = new Float32Array(@{typed_array}.buffer);
            var context = @{audio_ctxt};
            var buf_len = @{temporary_buffer.len() as u32};
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

        // TODO: handle latency better ; right now we just use setInterval with the amount of sound
        // data that is in each buffer ; this is obviously bad, and also the schedule is too tight
        // and there may be underflows
        set_timeout(|| audio_callback_fn::<D, E>(user_data_ptr), 330);
    }
}

impl Default for Devices {
    fn default() -> Devices {
        // We produce an empty iterator if the WebAudio API isn't available.
        Devices(is_webaudio_available())
    }
}
impl Iterator for Devices {
    type Item = Device;
    #[inline]
    fn next(&mut self) -> Option<Device> {
        if self.0 {
            self.0 = false;
            Some(Device)
        } else {
            None
        }
    }
}

#[inline]
fn default_input_device() -> Option<Device> {
    unimplemented!();
}

#[inline]
fn default_output_device() -> Option<Device> {
    if is_webaudio_available() {
        Some(Device)
    } else {
        None
    }
}

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
