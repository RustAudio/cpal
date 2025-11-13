use js_sys::Float32Array;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{AudioContext, AudioProcessingEvent, MediaStream, MediaStreamConstraints};

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};

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
    // A reference to an `AudioContext` object.
    audio_ctxt: AudioContext,
}

// WASM runs in a single-threaded environment, so Send is safe by design.
unsafe impl Send for Stream {}

// Compile-time assertion that Stream is Send
crate::assert_stream_send!(Stream);

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
const MAX_BUFFER_SIZE: u32 = u32::MAX;
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

/// Helper function to generate supported stream configurations.
/// Emscripten/WebAudio supports the same configurations for both input and output streams.
fn supported_configs() -> ::std::vec::IntoIter<SupportedStreamConfigRange> {
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
    configs.into_iter()
}

/// Helper function to get the default configuration.
/// Emscripten/WebAudio uses the same logic for both input and output.
fn default_config() -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
    let config = supported_configs()
        .max_by(|a, b| a.cmp_default_heuristics(b))
        .ok_or(DefaultStreamConfigError::DeviceNotAvailable)?
        .with_sample_rate(DEFAULT_SAMPLE_RATE);
    Ok(config)
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
        Ok(supported_configs())
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(supported_configs())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        default_config()
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        default_config()
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
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        build_input_stream_emscripten(config, sample_format, data_callback, error_callback)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
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
                if !(MIN_BUFFER_SIZE..=MAX_BUFFER_SIZE).contains(&v) {
                    return Err(BuildStreamError::StreamConfigNotSupported);
                }
                v as usize
            }
            BufferSize::Default => DEFAULT_BUFFER_SIZE,
        };

        // Create the stream.
        let audio_ctxt = AudioContext::new().expect("webaudio is not present on this system");
        let stream = Stream { audio_ctxt };

        // Use `set_timeout` to invoke a Rust callback repeatedly.
        //
        // The job of this callback is to fill the content of the audio buffers.
        //
        // See also: The call to `set_timeout` at the end of the `audio_callback_fn` which creates
        // the loop.
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

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let future = JsFuture::from(
            self.audio_ctxt
                .resume()
                .expect("Could not resume the stream"),
        );
        spawn_local(async {
            match future.await {
                Ok(value) => assert!(value.is_undefined()),
                Err(value) => panic!("AudioContext.resume() promise was rejected: {:?}", value),
            }
        });
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        let future = JsFuture::from(
            self.audio_ctxt
                .suspend()
                .expect("Could not suspend the stream"),
        );
        spawn_local(async {
            match future.await {
                Ok(value) => assert!(value.is_undefined()),
                Err(value) => panic!("AudioContext.suspend() promise was rejected: {:?}", value),
            }
        });
        Ok(())
    }
}

fn audio_callback_fn<D>(
    mut data_callback: D,
) -> impl FnOnce(Stream, StreamConfig, SampleFormat, u32)
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
{
    |stream, config, sample_format, buffer_size_frames| {
        let sample_rate = config.sample_rate.0;
        let buffer_size_samples = buffer_size_frames * config.channels as u32;
        let audio_ctxt = &stream.audio_ctxt;

        // TODO: We should be re-using a buffer.
        let mut temporary_buffer = vec![0f32; buffer_size_samples as usize];

        {
            let len = temporary_buffer.len();
            let data = temporary_buffer.as_mut_ptr() as *mut ();
            let mut data = unsafe { Data::from_parts(data, len, sample_format) };
            let now_secs: f64 = audio_ctxt.current_time();
            let callback = crate::StreamInstant::from_secs_f64(now_secs);
            // TODO: Use proper latency instead. Currently, unsupported on most browsers though, so
            // we estimate based on buffer size instead. Probably should use this, but it's only
            // supported by firefox (2020-04-28).
            // let latency_secs: f64 = audio_ctxt.outputLatency.try_into().unwrap();
            let buffer_duration = frames_to_duration(len, sample_rate as usize);
            let playback = callback
                .add(buffer_duration)
                .expect("`playback` occurs beyond representation supported by `StreamInstant`");
            let timestamp = crate::OutputStreamTimestamp { callback, playback };
            let info = OutputCallbackInfo { timestamp };
            data_callback(&mut data, &info);
        }

        let typed_array: Float32Array = temporary_buffer.as_slice().into();

        debug_assert_eq!(temporary_buffer.len() % config.channels as usize, 0);

        let src_buffer = Float32Array::new(typed_array.buffer().as_ref());
        let context = audio_ctxt;
        let buffer = context
            .create_buffer(
                config.channels as u32,
                buffer_size_frames as u32,
                sample_rate as f32,
            )
            .expect("Buffer could not be created");
        for channel in 0..config.channels {
            let mut buffer_content = buffer
                .get_channel_data(channel as u32)
                .expect("Should be impossible");
            for (i, buffer_content_item) in buffer_content.iter_mut().enumerate() {
                *buffer_content_item =
                    src_buffer.get_index(i as u32 * config.channels as u32 + channel as u32);
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
            stream.clone().clone(),
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
            &Closure::once_into_js(audio_callback_fn(data_callback))
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

fn build_input_stream_emscripten<D, E>(
    config: &StreamConfig,
    sample_format: SampleFormat,
    data_callback: D,
    error_callback: E,
) -> Result<Stream, BuildStreamError>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    if sample_format != SUPPORTED_SAMPLE_FORMAT {
        return Err(BuildStreamError::StreamConfigNotSupported);
    }

    let n_channels = config.channels as usize;
    let buffer_size_frames = match config.buffer_size {
        BufferSize::Fixed(v) => {
            if !(MIN_BUFFER_SIZE..=MAX_BUFFER_SIZE).contains(&v) {
                return Err(BuildStreamError::StreamConfigNotSupported);
            }
            v as usize
        }
        BufferSize::Default => DEFAULT_BUFFER_SIZE,
    };
    let buffer_size_samples = buffer_size_frames * n_channels;

    // Create the AudioContext
    let audio_ctxt = AudioContext::new().map_err(|err| {
        let description = format!("Failed to create AudioContext: {:?}", err);
        BuildStreamError::from(BackendSpecificError { description })
    })?;

    // Get the window and navigator objects
    let window = web_sys::window().ok_or_else(|| {
        let description = "Failed to get window object".to_string();
        BuildStreamError::from(BackendSpecificError { description })
    })?;

    let navigator = window.navigator();
    let media_devices = navigator.media_devices().map_err(|err| {
        let description = format!("Failed to get media devices: {:?}", err);
        BuildStreamError::from(BackendSpecificError { description })
    })?;

    // Create constraints for getUserMedia
    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::TRUE);
    constraints.set_video(&JsValue::FALSE);

    // Get the media stream asynchronously
    let get_user_media_promise = media_devices
        .get_user_media_with_constraints(&constraints)
        .map_err(|err| {
            let description = format!("Failed to call getUserMedia: {:?}", err);
            BuildStreamError::from(BackendSpecificError { description })
        })?;

    // Prepare variables that will be moved into the async closure
    let audio_ctxt_clone = audio_ctxt.clone();
    let data_callback = Arc::new(Mutex::new(data_callback));
    let error_callback = Arc::new(Mutex::new(error_callback));

    // Spawn async task to handle the getUserMedia Promise
    let future = async move {
        match JsFuture::from(get_user_media_promise).await {
            Ok(stream_js) => {
                // Convert JsValue to MediaStream
                let media_stream: MediaStream = match stream_js.dyn_into() {
                    Ok(stream) => stream,
                    Err(err) => {
                        let mut error_cb = error_callback.lock().unwrap();
                        let description = format!("Failed to convert to MediaStream: {:?}", err);
                        error_cb(StreamError::BackendSpecific {
                            err: BackendSpecificError { description },
                        });
                        return;
                    }
                };

                // Create MediaStreamAudioSourceNode
                let source = match audio_ctxt_clone.create_media_stream_source(&media_stream) {
                    Ok(s) => s,
                    Err(err) => {
                        let mut error_cb = error_callback.lock().unwrap();
                        let description =
                            format!("Failed to create MediaStreamAudioSourceNode: {:?}", err);
                        error_cb(StreamError::BackendSpecific {
                            err: BackendSpecificError { description },
                        });
                        return;
                    }
                };

                // Create ScriptProcessorNode for capturing audio
                let processor = match audio_ctxt_clone.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
                    buffer_size_frames as u32,
                    n_channels as u32,
                    0, // No output channels needed for input stream
                ) {
                    Ok(p) => p,
                    Err(err) => {
                        let mut error_cb = error_callback.lock().unwrap();
                        let description = format!("Failed to create ScriptProcessorNode: {:?}", err);
                        error_cb(StreamError::BackendSpecific {
                            err: BackendSpecificError { description },
                        });
                        return;
                    }
                };

                // Set up the onaudioprocess callback
                let mut temporary_buffer = vec![0f32; buffer_size_samples];
                let onaudioprocess_closure =
                    Closure::wrap(Box::new(move |event: AudioProcessingEvent| {
                        let input_buffer = match event.input_buffer() {
                            Ok(buf) => buf,
                            Err(_) => return, // Skip this callback if we can't get the buffer
                        };
                        let now = event.playback_time();

                        // Interleave the input channels into our temporary buffer
                        for channel in 0..n_channels {
                            if let Ok(channel_data) = input_buffer.get_channel_data(channel as u32)
                            {
                                for (i, sample) in channel_data.iter().enumerate() {
                                    if i < buffer_size_frames {
                                        temporary_buffer[i * n_channels + channel] = *sample;
                                    }
                                }
                            }
                        }

                        // Call the user's data callback
                        let len = temporary_buffer.len();
                        let data = temporary_buffer.as_mut_ptr() as *mut ();
                        let data = unsafe { Data::from_parts(data, len, sample_format) };
                        let mut callback = data_callback.lock().unwrap();
                        let capture = crate::StreamInstant::from_secs_f64(now);
                        let timestamp = crate::InputStreamTimestamp {
                            callback: capture,
                            capture,
                        };
                        let info = InputCallbackInfo { timestamp };
                        callback(&data, &info);
                    }) as Box<dyn FnMut(_)>);

                processor.set_onaudioprocess(Some(onaudioprocess_closure.as_ref().unchecked_ref()));
                onaudioprocess_closure.forget(); // Keep closure alive

                // Connect: source -> processor -> destination
                if let Err(err) = source.connect_with_audio_node(&processor) {
                    let mut error_cb = error_callback.lock().unwrap();
                    let description = format!("Failed to connect source to processor: {:?}", err);
                    error_cb(StreamError::BackendSpecific {
                        err: BackendSpecificError { description },
                    });
                    return;
                }

                // Connect processor to destination (required for onaudioprocess to fire in some browsers)
                let _ = processor.connect_with_audio_node(&audio_ctxt_clone.destination());
            }
            Err(err) => {
                let mut error_cb = error_callback.lock().unwrap();
                let description = format!("getUserMedia failed: {:?}", err);
                error_cb(StreamError::BackendSpecific {
                    err: BackendSpecificError { description },
                });
            }
        }
    };

    // Spawn the future
    spawn_local(future);

    Ok(Stream { audio_ctxt })
}

#[inline]
fn default_input_device() -> Option<Device> {
    if is_webaudio_available() {
        Some(Device)
    } else {
        None
    }
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
