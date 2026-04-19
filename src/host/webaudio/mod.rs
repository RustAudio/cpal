//! Web Audio backend implementation.
//!
//! Default backend on WebAssembly.

extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

use std::{
    ops::DerefMut,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use self::{
    wasm_bindgen::{prelude::*, JsCast},
    web_sys::{AudioContext, AudioContextOptions},
};
use crate::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceDirection,
    DeviceId, Error, ErrorKind, FrameCount, InputCallbackInfo, OutputCallbackInfo,
    OutputStreamTimestamp, SampleFormat, SampleRate, StreamConfig, StreamInstant,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
};

/// Type alias for shared closure handles used in audio callbacks
type ClosureHandle = Arc<RwLock<Option<Closure<dyn FnMut()>>>>;

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Device;

pub struct Host;

pub struct Stream {
    ctx: Arc<AudioContext>,
    on_ended_closures: Vec<ClosureHandle>,
    config: StreamConfig,
    buffer_size_frames: usize,
    is_started: Arc<AtomicBool>,
}

// WASM runs in a single-threaded environment, so Send and Sync are safe by design.
unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

// Compile-time assertion that Stream is Send and Sync
crate::assert_stream_send!(Stream);
crate::assert_stream_sync!(Stream);

pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

const MIN_CHANNELS: ChannelCount = 1;
const MAX_CHANNELS: ChannelCount = 32;
const MIN_SAMPLE_RATE: SampleRate = 8_000;
const MAX_SAMPLE_RATE: SampleRate = 96_000;
const DEFAULT_SAMPLE_RATE: SampleRate = 44_100;
const MIN_BUFFER_SIZE: u32 = 1;
const MAX_BUFFER_SIZE: u32 = u32::MAX;
const DEFAULT_BUFFER_SIZE: usize = 2048;
const SUPPORTED_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

impl Host {
    pub fn new() -> Result<Self, Error> {
        Ok(Self)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume this host is always available on webaudio.
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Self::Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
}

impl Devices {
    fn new() -> Result<Self, Error> {
        Ok(Self::default())
    }
}

impl Device {
    fn description(&self) -> Result<DeviceDescription, Error> {
        Ok(DeviceDescriptionBuilder::new("Default Device".to_string())
            .direction(DeviceDirection::Output)
            .build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId(
            crate::platform::HostId::WebAudio,
            "default".to_string(),
        ))
    }

    fn supported_input_configs(&self) -> Result<SupportedInputConfigs, Error> {
        // TODO
        Ok(Vec::new().into_iter())
    }

    fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        let buffer_size = SupportedBufferSize::Range {
            min: MIN_BUFFER_SIZE,
            max: MAX_BUFFER_SIZE,
        };
        let configs: Vec<_> = (MIN_CHANNELS..=MAX_CHANNELS)
            .map(|channels| SupportedStreamConfigRange {
                channels,
                min_sample_rate: MIN_SAMPLE_RATE,
                max_sample_rate: MAX_SAMPLE_RATE,
                buffer_size,
                sample_format: SUPPORTED_SAMPLE_FORMAT,
            })
            .collect();
        Ok(configs.into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Err(Error::with_message(
            ErrorKind::UnsupportedOperation,
            "WebAudio does not support audio input",
        ))
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
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

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Self::description(self)
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Self::id(self)
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Self::supported_input_configs(self)
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Self::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Self::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        Self::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _config: StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        Err(Error::with_message(
            ErrorKind::UnsupportedOperation,
            "WebAudio does not support audio input",
        ))
    }

    /// Create an output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if !valid_config(config, sample_format) {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "sample format {sample_format} or channel count {} is not supported by WebAudio",
                    config.channels
                ),
            ));
        }

        let n_channels = config.channels as usize;

        let buffer_size_frames = match config.buffer_size {
            BufferSize::Fixed(v) => {
                if !(MIN_BUFFER_SIZE..=MAX_BUFFER_SIZE).contains(&v) {
                    return Err(Error::with_message(
                        ErrorKind::UnsupportedConfig,
                        format!(
                            "buffer size {v} is out of the supported range {MIN_BUFFER_SIZE}..={MAX_BUFFER_SIZE}"
                        ),
                    ));
                }
                v as usize
            }
            BufferSize::Default => DEFAULT_BUFFER_SIZE,
        };
        let buffer_size_samples = buffer_size_frames * n_channels;
        let buffer_time_step_secs = buffer_time_step_secs(buffer_size_frames, config.sample_rate);

        let data_callback = Arc::new(Mutex::new(Box::new(data_callback)));
        let error_callback = Arc::new(Mutex::new(
            Box::new(error_callback) as Box<dyn FnMut(Error) + Send + 'static>
        ));
        let is_started = Arc::new(AtomicBool::new(false));

        // Create the WebAudio stream.
        let stream_opts = AudioContextOptions::new();
        stream_opts.set_sample_rate(config.sample_rate as f32);
        let ctx = AudioContext::new_with_context_options(&stream_opts)
            .map_err(|err| Error::with_message(ErrorKind::UnsupportedConfig, format!("{err:?}")))?;

        let destination = ctx.destination();

        // If possible, set the destination's channel_count to the given config.channel.
        // If not, fallback on the default destination channel_count to keep previous behavior
        // and do not return an error.
        if config.channels as u32 <= destination.max_channel_count() {
            destination.set_channel_count(config.channels as u32);
        }

        // SAFETY: WASM is single-threaded, so Arc is safe even though AudioContext is not Send/Sync
        #[allow(clippy::arc_with_non_send_sync)]
        let ctx = Arc::new(ctx);

        // A container for managing the lifecycle of the audio callbacks.
        let mut on_ended_closures: Vec<ClosureHandle> = Vec::new();

        // A cursor keeping track of the current time at which new frames should be scheduled.
        let time = Arc::new(RwLock::new(0f64));

        // baseLatency is fixed for the lifetime of the AudioContext.
        let base_latency_secs = js_sys::Reflect::get(ctx.as_ref(), &JsValue::from("baseLatency"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Create a set of closures / callbacks which will continuously fetch and schedule sample
        // playback. Starting with two workers, e.g. a front and back buffer so that audio frames
        // can be fetched in the background.
        for _i in 0..2 {
            let data_callback_handle = data_callback.clone();
            let error_callback_handle = error_callback.clone();
            let ctx_handle = ctx.clone();
            let time_handle = time.clone();

            // A set of temporary buffers to be used for intermediate sample transformation steps.
            let mut temporary_buffer = vec![0f32; buffer_size_samples];
            let mut temporary_channel_buffer = vec![0f32; buffer_size_frames];

            #[cfg(target_feature = "atomics")]
            let temporary_channel_array_view: js_sys::Float32Array;
            #[cfg(target_feature = "atomics")]
            {
                let temporary_channel_array = js_sys::ArrayBuffer::new(
                    (std::mem::size_of::<f32>() * buffer_size_frames) as u32,
                );
                temporary_channel_array_view = js_sys::Float32Array::new(&temporary_channel_array);
            }

            // Create a webaudio buffer which will be reused to avoid allocations.
            let ctx_buffer = ctx
                .create_buffer(
                    config.channels as u32,
                    buffer_size_frames as u32,
                    config.sample_rate as f32,
                )
                .map_err(|err| {
                    Error::with_message(ErrorKind::UnsupportedConfig, format!("{err:?}"))
                })?;

            // A self reference to this closure for passing to future audio event calls.
            // SAFETY: WASM is single-threaded, so Arc is safe even though Closure is not Send/Sync
            #[allow(clippy::arc_with_non_send_sync)]
            let on_ended_closure: ClosureHandle = Arc::new(RwLock::new(None));
            let on_ended_closure_handle = on_ended_closure.clone();

            on_ended_closure
                .write()
                .unwrap()
                .replace(Closure::wrap(Box::new(move || {
                    let now = ctx_handle.current_time();
                    let time_at_start_of_buffer = {
                        let time_at_start_of_buffer = time_handle
                            .read()
                            .expect("Unable to get a read lock on the time cursor");
                        // Synchronise first buffer as necessary (eg. keep the time value
                        // referenced to the context clock).
                        if *time_at_start_of_buffer > 0.0 {
                            *time_at_start_of_buffer
                        } else {
                            // Schedule the first buffer far enough ahead for the browser's
                            // internal audio pipeline (baseLatency) plus one full buffer of
                            // data, so playback starts underrun-free at any buffer size.
                            now + base_latency_secs + buffer_time_step_secs
                        }
                    };

                    // Populate the sample data into an interleaved temporary buffer.
                    {
                        let len = temporary_buffer.len();
                        let data = temporary_buffer.as_mut_ptr() as *mut ();
                        let mut data = unsafe { Data::from_parts(data, len, sample_format) };
                        match data_callback_handle.lock() {
                            Ok(mut data_callback) => {
                                // outputLatency can change at runtime, so read it each callback.
                                let output_latency_secs = js_sys::Reflect::get(
                                    ctx_handle.as_ref(),
                                    &JsValue::from("outputLatency"),
                                )
                                .ok()
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                                let total_hw_latency_secs = {
                                    let sum = base_latency_secs + output_latency_secs;
                                    if sum.is_finite() {
                                        sum.max(0.0)
                                    } else {
                                        0.0
                                    }
                                };
                                let callback = StreamInstant::from_secs_f64(now);
                                let playback = StreamInstant::from_secs_f64(
                                    time_at_start_of_buffer + total_hw_latency_secs,
                                );
                                let timestamp = OutputStreamTimestamp { callback, playback };
                                let info = OutputCallbackInfo { timestamp };
                                (data_callback.deref_mut())(&mut data, &info);
                            }
                            Err(_) => {
                                (error_callback_handle
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner()))(
                                    Error::with_message(
                                        ErrorKind::StreamInvalidated,
                                        "data callback lock poisoned",
                                    ),
                                );
                                return;
                            }
                        }
                    }

                    // Deinterleave the sample data and copy into the audio context buffer.
                    // We do not reference the audio context buffer directly e.g. getChannelData.
                    // As wasm-bindgen only gives us a copy, not a direct reference.
                    for channel in 0..n_channels {
                        for i in 0..buffer_size_frames {
                            temporary_channel_buffer[i] =
                                temporary_buffer[n_channels * i + channel];
                        }

                        #[cfg(not(target_feature = "atomics"))]
                        {
                            if let Err(err) = ctx_buffer
                                .copy_to_channel(&temporary_channel_buffer, channel as i32)
                            {
                                (error_callback_handle
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner()))(
                                    Error::with_message(
                                        ErrorKind::StreamInvalidated,
                                        format!("{err:?}"),
                                    ),
                                );
                                return;
                            }
                        }

                        // copyToChannel cannot be directly copied into from a SharedArrayBuffer,
                        // which WASM memory is backed by if the 'atomics' flag is enabled.
                        // This workaround copies the data into an intermediary buffer first.
                        // There's a chance browsers may eventually relax that requirement.
                        // See this issue: https://github.com/WebAudio/web-audio-api/issues/2565
                        #[cfg(target_feature = "atomics")]
                        {
                            temporary_channel_array_view.copy_from(&temporary_channel_buffer);
                            if let Err(err) = ctx_buffer
                                .unchecked_ref::<ExternalArrayAudioBuffer>()
                                .copy_to_channel(&temporary_channel_array_view, channel as i32)
                            {
                                (error_callback_handle
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner()))(
                                    Error::with_message(
                                        ErrorKind::StreamInvalidated,
                                        format!("{err:?}"),
                                    ),
                                );
                                return;
                            }
                        }
                    }

                    // Create an AudioBufferSourceNode, schedule it to playback the reused buffer
                    // in the future.
                    let source = match ctx_handle.create_buffer_source() {
                        Ok(s) => s,
                        Err(err) => {
                            // create_buffer_source is documented not to throw; defensive only.
                            (error_callback_handle
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()))(
                                Error::with_message(
                                    ErrorKind::StreamInvalidated,
                                    format!("{err:?}"),
                                ),
                            );
                            return;
                        }
                    };
                    source.set_buffer(Some(&ctx_buffer));
                    if let Err(err) = source.connect_with_audio_node(&ctx_handle.destination()) {
                        (error_callback_handle
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()))(
                            Error::with_message(ErrorKind::StreamInvalidated, format!("{err:?}")),
                        );
                        return;
                    }
                    if let Err(err) = source.add_event_listener_with_callback(
                        "ended",
                        on_ended_closure_handle
                            .read()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .as_ref()
                            .unchecked_ref(),
                    ) {
                        // addEventListener is documented not to throw; defensive only.
                        (error_callback_handle
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()))(
                            Error::with_message(ErrorKind::StreamInvalidated, format!("{err:?}")),
                        );
                        return;
                    }
                    if let Err(err) = source.start_with_when(time_at_start_of_buffer) {
                        // InvalidStateError (already started) is the expected failure mode.
                        (error_callback_handle
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()))(
                            Error::with_message(ErrorKind::StreamInvalidated, format!("{err:?}")),
                        );
                        return;
                    }

                    // Keep track of when the next buffer worth of samples should be played.
                    *time_handle.write().unwrap() = time_at_start_of_buffer + buffer_time_step_secs;
                }) as Box<dyn FnMut()>));

            on_ended_closures.push(on_ended_closure);
        }

        Ok(Self::Stream {
            ctx,
            on_ended_closures,
            config,
            buffer_size_frames,
            is_started,
        })
    }
}

impl Stream {
    /// Return the [`AudioContext`](https://developer.mozilla.org/docs/Web/API/AudioContext) used
    /// by this stream.
    pub fn audio_context(&self) -> &AudioContext {
        &self.ctx
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        let window = web_sys::window().unwrap();
        match self.ctx.resume() {
            Ok(_) => {
                // Only schedule the initial timeouts once.
                if self
                    .is_started
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    return Ok(());
                }
                // Begin webaudio playback, initially scheduling the closures to fire on a timeout
                // event. Minimum value as per spec: https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#timers
                let mut offset_ms = 4;
                let time_step_secs =
                    buffer_time_step_secs(self.buffer_size_frames, self.config.sample_rate);
                let time_step_ms = ((time_step_secs * 1_000.0).ceil() as i32).max(1);
                for on_ended_closure in self.on_ended_closures.iter() {
                    window
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            on_ended_closure
                                .read()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .as_ref()
                                .unchecked_ref(),
                            offset_ms,
                        )
                        .unwrap();
                    offset_ms += time_step_ms;
                }
                Ok(())
            }
            Err(err) => Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                format!("{err:?}"),
            )),
        }
    }

    fn pause(&self) -> Result<(), Error> {
        match self.ctx.suspend() {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                format!("{err:?}"),
            )),
        }
    }

    fn now(&self) -> StreamInstant {
        StreamInstant::from_secs_f64(self.ctx.current_time())
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.buffer_size_frames as FrameCount)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.ctx.close();
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

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 {
            self.0 = false;
            Some(Device)
        } else {
            None
        }
    }
}

fn default_input_device() -> Option<Device> {
    // TODO
    None
}

fn default_output_device() -> Option<Device> {
    if is_webaudio_available() {
        Some(Device)
    } else {
        None
    }
}

// Detects whether the `AudioContext` global variable is available.
fn is_webaudio_available() -> bool {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from("AudioContext"))
        .unwrap()
        .is_truthy()
}

// Whether or not the given stream configuration is valid for building a stream.
fn valid_config(conf: StreamConfig, sample_format: SampleFormat) -> bool {
    conf.channels <= MAX_CHANNELS
        && conf.channels >= MIN_CHANNELS
        && conf.sample_rate <= MAX_SAMPLE_RATE
        && conf.sample_rate >= MIN_SAMPLE_RATE
        && sample_format == SUPPORTED_SAMPLE_FORMAT
}

fn buffer_time_step_secs(buffer_size_frames: usize, sample_rate: SampleRate) -> f64 {
    buffer_size_frames as f64 / sample_rate as f64
}

#[cfg(target_feature = "atomics")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = AudioBuffer)]
    type ExternalArrayAudioBuffer;

    # [wasm_bindgen(catch, method, structural, js_class = "AudioBuffer", js_name = copyToChannel)]
    pub fn copy_to_channel(
        this: &ExternalArrayAudioBuffer,
        source: &js_sys::Float32Array,
        channel_number: i32,
    ) -> Result<(), JsValue>;
}
