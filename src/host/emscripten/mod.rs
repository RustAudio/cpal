use js_sys::Float32Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::AudioContext;

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

#[wasm_bindgen]
#[derive(Clone)]
pub struct Stream {
    audio_ctxt: AudioContext,
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
        _error_callback: E,
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

        let audio_ctxt = AudioContext::new().expect("webaudio is not present on the system");
        let stream = Stream { audio_ctxt };
        set_timeout(
            10,
            stream.clone(),
            data_callback,
            config,
            sample_format,
            buffer_size_frames as u32,
        );
        Ok(stream)
    }
}

// TODO: Do something useful with the values, maybe through wasm_bindgen_futures
impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.audio_ctxt
            .resume()
            .expect("Could not resume the stream");
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.audio_ctxt
            .suspend()
            .expect("Could not suspend the stream");
        Ok(())
    }
}

fn callback_fn<D>(
    mut data_callback: D,
) -> impl FnOnce(Stream, StreamConfig, SampleFormat, usize) + 'static
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
{
    |stream: Stream,
     config: StreamConfig,
     sample_format: SampleFormat,
     buffer_size_frames: usize| {
        let num_channels = config.channels as usize;
        let sample_rate = config.sample_rate.0;
        let buffer_size_samples = buffer_size_frames * num_channels;
        let audio_ctxt = &stream.audio_ctxt;

        // TODO: We should be re-using a buffer.
        let mut temporary_buffer = vec![0f32; buffer_size_samples];

        {
            let len = temporary_buffer.len();
            let data = temporary_buffer.as_mut_ptr() as *mut ();
            let mut data = unsafe { Data::from_parts(data, len, sample_format) };
            let now_secs: f64 = audio_ctxt.current_time();
            let callback = crate::StreamInstant::from_secs_f64(now_secs);
            let buffer_duration = frames_to_duration(len, sample_rate as usize);
            let playback = callback
                .add(buffer_duration)
                .expect("`playback` occurs beyond representation supported by `StreamInstant`");
            let timestamp = crate::OutputStreamTimestamp { callback, playback };
            let info = OutputCallbackInfo { timestamp };
            data_callback(&mut data, &info);
        }

        let typed_array: Float32Array = temporary_buffer.as_slice().into();

        debug_assert_eq!(temporary_buffer.len() % num_channels as usize, 0);
        let src_buffer = Float32Array::new(typed_array.buffer().as_ref());
        let context = audio_ctxt;
        let buffer = context
            .create_buffer(
                num_channels as u32,
                buffer_size_frames as u32,
                sample_rate as f32,
            )
            .expect("Buffer could not be created");
        for channel in 0..num_channels {
            let mut buffer_content = buffer
                .get_channel_data(channel as u32)
                .expect("Should be impossible");
            for (i, buffer_content_item) in buffer_content.iter_mut().enumerate() {
                *buffer_content_item = src_buffer.get_index((i * num_channels + channel) as u32);
            }
        }

        let node = context
            .create_buffer_source()
            .expect("The buffer source node could not be created");
        node.set_buffer(Some(&buffer));
        context
            .destination()
            .connect_with_audio_node(&node)
            .expect("Could not connect the audio node to the destination");
        node.start().expect("Could not start the audio node");

        // TODO: handle latency better ; right now we just use setInterval with the amount of sound
        // data that is in each buffer ; this is obviously bad, and also the schedule is too tight
        // and there may be underflows
        set_timeout(
            1000 * buffer_size_frames as i32 / sample_rate as i32,
            stream,
            data_callback,
            &config,
            sample_format,
            buffer_size_frames as u32,
        );
    }
}

fn set_timeout<D>(
    time: i32,
    stream: Stream,
    data_callback: D,
    config: &StreamConfig,
    sample_format: SampleFormat,
    buffer_size_frames: u32,
) where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
{
    let window = web_sys::window().expect("Not in a window somehow?");
    window
        .set_timeout_with_callback_and_timeout_and_arguments_4(
            &Closure::once_into_js(callback_fn(data_callback))
                .dyn_ref::<js_sys::Function>()
                .expect("The function was somehow not a function"),
            time,
            &stream.into(),
            &((*config).clone()).into(),
            &Closure::once_into_js(move || sample_format),
            &buffer_size_frames.into(),
        )
        .expect("The timeout could not be set");
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
    AudioContext::new().is_ok()
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
