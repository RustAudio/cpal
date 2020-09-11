use std::mem;
use std::os::raw::c_void;
use std::slice::from_raw_parts;
use stdweb;
use stdweb::unstable::TryInto;
use stdweb::web::set_timeout;
use stdweb::web::TypedArray;
use stdweb::Reference;

use crate::{
    BufferSize, BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat,
    SampleRate, StreamConfig, StreamError, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange, SupportedStreamConfigsError,
};
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

pub type SupportedInputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;

const MIN_CHANNELS: u16 = 1;
const MAX_CHANNELS: u16 = 32;
const MIN_SAMPLE_RATE: SampleRate = SampleRate(8_000);
const MAX_SAMPLE_RATE: SampleRate = SampleRate(96_000);
const DEFAULT_SAMPLE_RATE: SampleRate = SampleRate(44_100);
const MIN_BUFFER_SIZE: u32 = 1;
const MAX_BUFFER_SIZE: u32 = std::u32::MAX;
const DEFAULT_BUFFER_SIZE: usize = 2048;
const SUPPORTED_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

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
    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        unimplemented!();
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        let buffer_size = SupportedBufferSize::Range {
            min: MIN_BUFFER_SIZE,
            max: MAX_BUFFER_SIZE,
        };
        let configs: Vec<_> = (MIN_CHANNELS..=MAX_CHANNELS)
            .map(|channels| SupportedStreamConfigRange {
                channels,
                min_sample_rate: MIN_SAMPLE_RATE,
                max_sample_rate: MAX_SAMPLE_RATE,
                buffer_size: buffer_size.clone(),
                sample_format: SUPPORTED_SAMPLE_FORMAT,
            })
            .collect();
        Ok(configs.into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        unimplemented!();
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        const EXPECT: &str = "expected at least one valid webaudio stream config";
        let config = self
            .supported_output_configs()
            .expect(EXPECT)
            .max_by(|a, b| a.cmp_default_heuristics(b))
            .unwrap()
            .with_sample_rate(DEFAULT_SAMPLE_RATE);

        Ok(config)
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
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Device::supported_input_configs(self)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Device::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _config: &StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        unimplemented!()
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        if !valid_config(config, sample_format) {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let buffer_size_frames = match config.buffer_size {
            BufferSize::Fixed(v) => {
                if v == 0 {
                    return Err(BuildStreamError::StreamConfigNotSupported);
                } else {
                    v as usize
                }
            }
            BufferSize::Default => DEFAULT_BUFFER_SIZE,
        };

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
        set_timeout(
            || {
                audio_callback_fn::<D, E>(
                    user_data_ptr as *mut c_void,
                    config,
                    sample_format,
                    buffer_size_frames,
                )
            },
            10,
        );

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
fn audio_callback_fn<D, E>(
    user_data_ptr: *mut c_void,
    config: &StreamConfig,
    sample_format: SampleFormat,
    buffer_size_frames: usize,
) where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    let num_channels = config.channels as usize;
    let sample_rate = config.sample_rate.0;
    let buffer_size_samples = buffer_size_frames * num_channels;

    unsafe {
        let user_data_ptr2 = user_data_ptr as *mut (&Stream, D, E);
        let user_data = &mut *user_data_ptr2;
        let (ref stream, ref mut data_cb, ref mut _err_cb) = user_data;
        let audio_ctxt = &stream.audio_ctxt_ref;

        // TODO: We should be re-using a buffer.
        let mut temporary_buffer = vec![0f32; buffer_size_samples];

        {
            let len = temporary_buffer.len();
            let data = temporary_buffer.as_mut_ptr() as *mut ();
            let mut data = Data::from_parts(data, len, sample_format);

            let now_secs: f64 = js!(@{audio_ctxt}.getOutputTimestamp().currentTime)
                .try_into()
                .expect("failed to retrieve Value as f64");
            let callback = crate::StreamInstant::from_secs_f64(now_secs);
            // TODO: Use proper latency instead. Currently unsupported on most browsers though so
            // we estimate based on buffer size instead. Probably should use this, but it's only
            // supported by firefox (2020-04-28).
            // let latency_secs: f64 = js!(@{audio_ctxt}.outputLatency).try_into().unwrap();
            let buffer_duration = frames_to_duration(len, sample_rate as usize);
            let playback = callback
                .add(buffer_duration)
                .expect("`playback` occurs beyond representation supported by `StreamInstant`");
            let timestamp = crate::OutputStreamTimestamp { callback, playback };
            let info = OutputCallbackInfo { timestamp };
            data_cb(&mut data, &info);
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

        debug_assert_eq!(temporary_buffer.len() % num_channels as usize, 0);

        js!(
            var src_buffer = new Float32Array(@{typed_array}.buffer);
            var context = @{audio_ctxt};
            var buffer_size_frames = @{buffer_size_frames as u32};
            var num_channels = @{num_channels as u32};
            var sample_rate = sample_rate;

            var buffer = context.createBuffer(num_channels, buffer_size_frames, sample_rate);
            for (var channel = 0; channel < num_channels; ++channel) {
                var buffer_content = buffer.getChannelData(channel);
                for (var i = 0; i < buffer_size_frames; ++i) {
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
        set_timeout(
            || audio_callback_fn::<D, E>(user_data_ptr, config, sample_format, buffer_size_frames),
            buffer_size_frames as u32 * 1000 / sample_rate,
        );
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
    })
    .try_into()
    .unwrap()
}

// Whether or not the given stream configuration is valid for building a stream.
fn valid_config(conf: &StreamConfig, sample_format: SampleFormat) -> bool {
    conf.channels <= MAX_CHANNELS
        && conf.channels >= MIN_CHANNELS
        && conf.sample_rate <= MAX_SAMPLE_RATE
        && conf.sample_rate >= MIN_SAMPLE_RATE
        && sample_format == SUPPORTED_SAMPLE_FORMAT
}

// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: usize, rate: usize) -> std::time::Duration {
    let secsf = frames as f64 / rate as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}
