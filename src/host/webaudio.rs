//! Web Audio backend implementation.
//!
//! Default backend on WebAssembly.

extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

#[cfg(target_feature = "atomics")]
use std::sync::atomic::AtomicU64;
use std::{
    fmt,
    ops::DerefMut,
    sync::{atomic::Ordering, Arc, Mutex, RwLock},
    time::Duration,
};

#[cfg(not(target_feature = "atomics"))]
use std::sync::atomic::AtomicBool;

#[cfg(target_feature = "atomics")]
use futures_channel::mpsc;
#[cfg(target_feature = "atomics")]
use futures_util::StreamExt as _;

type OutputDataCallbackArc = Arc<Mutex<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send>>;

use self::{
    wasm_bindgen::{prelude::*, JsCast},
    web_sys::{AudioContext, AudioContextOptions},
};
use crate::{
    host::ErrorCallbackArc,
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceDirection,
    DeviceId, Error, ErrorKind, FrameCount, InputCallbackInfo, OutputCallbackInfo,
    OutputStreamTimestamp, Sample, SampleFormat, SampleRate, StreamConfig, StreamInstant,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
};

/// Type alias for shared closure handles used in audio callbacks
type ClosureHandle = Arc<RwLock<Option<Closure<dyn FnMut()>>>>;

#[cfg(target_feature = "atomics")]
enum Command {
    Play,
    Pause,
}

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Device;

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = self.description().map_err(|_| fmt::Error)?;
        f.write_str(desc.name())
    }
}

pub struct Host;

pub struct Stream {
    buffer_size_frames: usize,

    // Single-threaded WASM: hold JS types directly. Safe because there is only one thread.
    #[cfg(not(target_feature = "atomics"))]
    config: StreamConfig,
    #[cfg(not(target_feature = "atomics"))]
    ctx: Arc<AudioContext>,
    #[cfg(not(target_feature = "atomics"))]
    on_ended_closures: Vec<ClosureHandle>,
    #[cfg(not(target_feature = "atomics"))]
    is_started: Arc<AtomicBool>,

    // Multi-threaded WASM (+atomics): all fields are Send+Sync; JS types are owned by a
    // spawn_local future on the local thread and are never stored here.
    #[cfg(target_feature = "atomics")]
    command_tx: mpsc::UnboundedSender<Command>,
    #[cfg(target_feature = "atomics")]
    current_time_bits: Arc<AtomicU64>,
}

// Without atomics, WASM is single-threaded, so there are no thread boundaries to cross.
#[cfg(not(target_feature = "atomics"))]
unsafe impl Send for Stream {}
#[cfg(not(target_feature = "atomics"))]
unsafe impl Sync for Stream {}
// With atomics, all Stream fields auto-derive Send+Sync.

// Compile-time assertion that Stream is Send and Sync
crate::assert_stream_send!(Stream);
crate::assert_stream_sync!(Stream);

pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

// https://webaudio.github.io/web-audio-api/#dom-baseaudiocontext-createbuffer
const MIN_CHANNELS: ChannelCount = 1;
const MAX_CHANNELS: ChannelCount = 32;

// https://webaudio.github.io/web-audio-api/#supported-sample-rates
const MIN_SAMPLE_RATE: SampleRate = 3_000;
const MAX_SAMPLE_RATE: SampleRate = 768_000;

// https://webaudio.github.io/web-audio-api/#audio-processing-model
const SUPPORTED_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

const DEFAULT_BUFFER_SIZE: usize = 2048;

// Minimum initial timer delay mandated by the HTML spec.
// https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#timers
const INITIAL_TIMEOUT_MS: i32 = 4;

impl Host {
    pub fn new() -> Result<Self, Error> {
        if Self::is_available() {
            Ok(Self)
        } else {
            Err(Error::with_message(
                ErrorKind::HostUnavailable,
                "WebAudio is not available in this context",
            ))
        }
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        is_webaudio_available()
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
        Ok(Devices(is_webaudio_available()))
    }
}

impl Device {
    fn description(&self) -> Result<DeviceDescription, Error> {
        Ok(DeviceDescriptionBuilder::new("Default Device")
            .direction(DeviceDirection::Output)
            .build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId::new(crate::platform::HostId::WebAudio, "default"))
    }

    fn supported_input_configs(&self) -> Result<SupportedInputConfigs, Error> {
        // TODO
        Ok(Vec::new().into_iter())
    }

    fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        let buffer_size = SupportedBufferSize::Range {
            min: 1,
            max: FrameCount::MAX,
        };
        let configs: Vec<_> = (MIN_CHANNELS..=MAX_CHANNELS)
            .flat_map(|channels| {
                crate::COMMON_SAMPLE_RATES
                    .iter()
                    .copied()
                    .filter(|&r| (MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&r))
                    .map(move |rate| SupportedStreamConfigRange {
                        channels,
                        min_sample_rate: rate,
                        max_sample_rate: rate,
                        buffer_size,
                        sample_format: SUPPORTED_SAMPLE_FORMAT,
                    })
            })
            .collect();
        Ok(configs.into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Err(Error::with_message(
            ErrorKind::UnsupportedOperation,
            "Device does not support input",
        ))
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        let range = self
            .supported_output_configs()?
            .max_by(|a, b| a.cmp_default_heuristics(b))
            .ok_or_else(|| {
                Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    "No supported output configuration",
                )
            })?;
        let config = range
            .try_with_standard_sample_rate()
            .unwrap_or_else(|| range.with_max_sample_rate());

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
            "Device does not support input",
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
        crate::validate_stream_config(&config)?;
        if config.channels > MAX_CHANNELS {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "Channel count {} exceeds the maximum of {MAX_CHANNELS}",
                    config.channels
                ),
            ));
        }
        if sample_format != SUPPORTED_SAMPLE_FORMAT {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "Sample format {sample_format} is not supported; required format is {SUPPORTED_SAMPLE_FORMAT}"
                ),
            ));
        }
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&config.sample_rate) {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "Sample rate {} Hz is not in the supported range {MIN_SAMPLE_RATE}..={MAX_SAMPLE_RATE} Hz",
                    config.sample_rate
                ),
            ));
        }

        let n_channels = config.channels as usize;

        let buffer_size_frames = match config.buffer_size {
            BufferSize::Fixed(v) => v as usize,
            BufferSize::Default => DEFAULT_BUFFER_SIZE,
        };
        let buffer_size_samples = buffer_size_frames.checked_mul(n_channels).ok_or_else(|| {
            Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "Buffer size {} * channel count {} overflows on this platform",
                    buffer_size_frames, config.channels
                ),
            )
        })?;
        let buffer_time_step_secs = buffer_time_step_secs(buffer_size_frames, config.sample_rate);

        // Keep `playback` monotonic: outputLatency can drop (e.g. the page calls `setSinkId()` to
        // switch output devices), which would pull `playback` backward.
        let data_callback = crate::host::monotonic_output_callback(data_callback);
        let data_callback: OutputDataCallbackArc = Arc::new(Mutex::new(data_callback));
        let error_callback: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));

        #[cfg(not(target_feature = "atomics"))]
        let is_started = Arc::new(AtomicBool::new(false));

        // Create the WebAudio stream.
        let stream_opts = AudioContextOptions::new();
        stream_opts.set_sample_rate(config.sample_rate as f32);
        let ctx = AudioContext::new_with_context_options(&stream_opts).map_err(|_| {
            Error::with_message(
                ErrorKind::UnsupportedConfig,
                "Failed to create audio context",
            )
        })?;

        let destination = ctx.destination();

        if config.channels as u32 > destination.max_channel_count() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "Channel count {} exceeds the destination's maximum of {}",
                    config.channels,
                    destination.max_channel_count()
                ),
            ));
        }
        destination.set_channel_count(config.channels as u32);

        // SAFETY: AudioContext and Closure are not Send/Sync. In the non-atomics path WASM is
        // single-threaded so there are no thread boundaries to cross. In the atomics path these
        // values are moved into a spawn_local future on the same local thread; they are never
        // stored in Stream (which is Send+Sync) and therefore never escape to another thread.
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

        // Shared current-time counter updated on every callback invocation.
        // Seeded from the live clock so now() is on the correct time base before the first callback.
        #[cfg(target_feature = "atomics")]
        let current_time_bits = Arc::new(AtomicU64::new(ctx.current_time().to_bits()));

        // Create a set of closures / callbacks which will continuously fetch and schedule sample
        // playback. Starting with two workers, e.g. a front and back buffer so that audio frames
        // can be fetched in the background.
        for _i in 0..2 {
            let data_callback_handle = data_callback.clone();
            let error_callback_handle = error_callback.clone();
            let ctx_handle = ctx.clone();
            let time_handle = time.clone();

            #[cfg(target_feature = "atomics")]
            let current_time_bits_handle = current_time_bits.clone();

            // A set of temporary buffers to be used for intermediate sample transformation steps.
            let mut temporary_buffer = vec![f32::EQUILIBRIUM; buffer_size_samples];
            let mut temporary_channel_buffer = vec![f32::EQUILIBRIUM; buffer_size_frames];

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
                .map_err(|_| {
                    Error::with_message(
                        ErrorKind::UnsupportedConfig,
                        "Failed to create audio buffer",
                    )
                })?;

            // A self reference to this closure for passing to future audio event calls.
            #[allow(clippy::arc_with_non_send_sync)]
            let on_ended_closure: ClosureHandle = Arc::new(RwLock::new(None));
            let on_ended_closure_handle = on_ended_closure.clone();

            on_ended_closure
                .write()
                .unwrap()
                .replace(Closure::wrap(Box::new(move || {
                    let now = ctx_handle.current_time();

                    // Keep the shared clock up to date so Stream::now() has a fresh value.
                    #[cfg(target_feature = "atomics")]
                    current_time_bits_handle.store(now.to_bits(), Ordering::Relaxed);

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
                        temporary_buffer.fill(f32::EQUILIBRIUM);
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
                                        "Stream lock poisoned",
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
                            if ctx_buffer
                                .copy_to_channel(&temporary_channel_buffer, channel as i32)
                                .is_err()
                            {
                                (error_callback_handle
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner()))(
                                    Error::with_message(
                                        ErrorKind::StreamInvalidated,
                                        "Failed to copy audio data",
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
                            if ctx_buffer
                                .unchecked_ref::<ExternalArrayAudioBuffer>()
                                .copy_to_channel(&temporary_channel_array_view, channel as i32)
                                .is_err()
                            {
                                (error_callback_handle
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner()))(
                                    Error::with_message(
                                        ErrorKind::StreamInvalidated,
                                        "Failed to copy audio data",
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
                        Err(_) => {
                            // create_buffer_source is documented not to throw; defensive only.
                            (error_callback_handle
                                .lock()
                                .unwrap_or_else(|e| e.into_inner()))(
                                Error::with_message(
                                    ErrorKind::StreamInvalidated,
                                    "Failed to create audio buffer source",
                                ),
                            );
                            return;
                        }
                    };
                    source.set_buffer(Some(&ctx_buffer));
                    if source
                        .connect_with_audio_node(&ctx_handle.destination())
                        .is_err()
                    {
                        (error_callback_handle
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()))(
                            Error::with_message(
                                ErrorKind::StreamInvalidated,
                                "Failed to connect audio node",
                            ),
                        );
                        return;
                    }
                    if source
                        .add_event_listener_with_callback(
                            "ended",
                            on_ended_closure_handle
                                .read()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .as_ref()
                                .unchecked_ref(),
                        )
                        .is_err()
                    {
                        // addEventListener is documented not to throw; defensive only.
                        (error_callback_handle
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()))(
                            Error::with_message(
                                ErrorKind::StreamInvalidated,
                                "Failed to register audio event listener",
                            ),
                        );
                        return;
                    }
                    if source.start_with_when(time_at_start_of_buffer).is_err() {
                        // InvalidStateError (already started) is the expected failure mode.
                        (error_callback_handle
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()))(
                            Error::with_message(
                                ErrorKind::StreamInvalidated,
                                "Failed to start audio buffer source",
                            ),
                        );
                        return;
                    }

                    // Keep track of when the next buffer worth of samples should be played.
                    *time_handle.write().unwrap() = time_at_start_of_buffer + buffer_time_step_secs;
                }) as Box<dyn FnMut()>));

            on_ended_closures.push(on_ended_closure);
        }

        #[cfg(not(target_feature = "atomics"))]
        {
            Ok(Self::Stream {
                ctx,
                on_ended_closures,
                config,
                buffer_size_frames,
                is_started,
            })
        }

        #[cfg(target_feature = "atomics")]
        {
            let current_time_bits_stream = current_time_bits.clone();
            let (command_tx, mut command_rx) = mpsc::unbounded::<Command>();
            wasm_bindgen_futures::spawn_local(async move {
                let window = web_sys::window().unwrap();
                let mut started = false;
                while let Some(cmd) = command_rx.next().await {
                    match cmd {
                        Command::Play => {
                            if ctx.resume().is_err() {
                                error_callback.lock().unwrap_or_else(|e| e.into_inner())(
                                    Error::with_message(
                                        ErrorKind::DeviceNotAvailable,
                                        "Failed to resume audio context",
                                    ),
                                );
                            } else if !started {
                                started = true;
                                schedule_initial_timeouts(
                                    &window,
                                    &on_ended_closures,
                                    buffer_size_frames,
                                    config.sample_rate,
                                );
                            }
                        }
                        Command::Pause => {
                            if ctx.suspend().is_err() {
                                error_callback.lock().unwrap_or_else(|e| e.into_inner())(
                                    Error::with_message(
                                        ErrorKind::DeviceNotAvailable,
                                        "Failed to suspend audio context",
                                    ),
                                );
                            }
                        }
                    }
                }
                // Stream dropped: close the AudioContext on the main thread.
                let _ = ctx.close();
            });
            Ok(Self::Stream {
                command_tx,
                current_time_bits: current_time_bits_stream,
                buffer_size_frames,
            })
        }
    }
}

// Without atomics: AudioContext is accessible directly from Stream.
#[cfg(not(target_feature = "atomics"))]
impl Stream {
    /// Return the [`AudioContext`](https://developer.mozilla.org/docs/Web/API/AudioContext) used
    /// by this stream.
    pub fn audio_context(&self) -> &AudioContext {
        &self.ctx
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        #[cfg(not(target_feature = "atomics"))]
        {
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
                    schedule_initial_timeouts(
                        &window,
                        &self.on_ended_closures,
                        self.buffer_size_frames,
                        self.config.sample_rate,
                    );
                    Ok(())
                }
                Err(_) => Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    "Failed to resume audio context",
                )),
            }
        }
        #[cfg(target_feature = "atomics")]
        self.command_tx.unbounded_send(Command::Play).map_err(|_| {
            Error::with_message(
                ErrorKind::StreamInvalidated,
                "WebAudio context task stopped unexpectedly",
            )
        })
    }

    fn pause(&self) -> Result<(), Error> {
        #[cfg(not(target_feature = "atomics"))]
        {
            match self.ctx.suspend() {
                Ok(_) => Ok(()),
                Err(_) => Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    "Failed to suspend audio context",
                )),
            }
        }
        #[cfg(target_feature = "atomics")]
        self.command_tx.unbounded_send(Command::Pause).map_err(|_| {
            Error::with_message(
                ErrorKind::StreamInvalidated,
                "WebAudio context task stopped unexpectedly",
            )
        })
    }

    fn now(&self) -> StreamInstant {
        #[cfg(not(target_feature = "atomics"))]
        let t = self.ctx.current_time();
        #[cfg(target_feature = "atomics")]
        let t = f64::from_bits(self.current_time_bits.load(Ordering::Relaxed));
        StreamInstant::from_secs_f64(t)
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.buffer_size_frames as FrameCount)
    }
}

// Without atomics: close the AudioContext synchronously on drop.
#[cfg(not(target_feature = "atomics"))]
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.ctx.close();
    }
}
// With atomics: dropping `command_tx` closes the channel, which signals the
// spawn_local task to call ctx.close() on the main thread.

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
    if Host::is_available() {
        Some(Device)
    } else {
        None
    }
}

// Detects whether WebAudio is available: requires a window context (not a Worker) with an
// AudioContext constructor present.
fn is_webaudio_available() -> bool {
    web_sys::window()
        .and_then(|w| js_sys::Reflect::get(w.as_ref(), &JsValue::from("AudioContext")).ok())
        .is_some_and(|v| v.is_truthy())
}

fn buffer_time_step_secs(buffer_size_frames: usize, sample_rate: SampleRate) -> f64 {
    buffer_size_frames as f64 / sample_rate as f64
}

/// Stagger the initial `setTimeout` kicks for the double-buffer pair so the two closures
/// don't fire at the same instant on the first tick.
fn schedule_initial_timeouts(
    window: &web_sys::Window,
    on_ended_closures: &[ClosureHandle],
    buffer_size_frames: usize,
    sample_rate: SampleRate,
) {
    let time_step_secs = buffer_time_step_secs(buffer_size_frames, sample_rate);
    let time_step_ms = ((time_step_secs * 1_000.0).ceil() as i32).max(1);
    let mut offset_ms = INITIAL_TIMEOUT_MS;
    for on_ended_closure in on_ended_closures {
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
