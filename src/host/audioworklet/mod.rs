//! Audio Worklet backend implementation.
//!
//! Available on WebAssembly with the `audioworklet` feature. Requires atomics support.
//! See the `audioworklet` example for setup instructions.

use std::{
    cell::RefCell,
    fmt,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_channel::mpsc;
use futures_util::StreamExt as _;
use js_sys::wasm_bindgen;
use wasm_bindgen::prelude::*;

use crate::{
    BufferSize, CallbackInfo, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder,
    DeviceDirection, DeviceId, DuplexCallbackInfo, DuplexStreamConfig, Error, ErrorKind,
    FrameCount, Sample, SampleFormat, SampleRate, StreamConfig, StreamInstant, StreamTimestamp,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    host::frames_to_duration,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

mod dependent_module;
use crate::dependent_module;

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

// `Stream`'s fields are all Send+Sync (a channel sender plus atomics).
// Any JS-backed resource (e.g. `web_sys::Window`, `Closure`) must live as a local
// inside the `spawn_local` task in `build_*_stream_raw`.
pub struct Stream {
    command_tx: mpsc::UnboundedSender<Command>,
    current_time_bits: Arc<AtomicU64>,
    buffer_size_frames: Arc<AtomicU64>,
}

/// How often the main thread re-reads `outputLatency` to publish it to the worklet.
const LATENCY_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

// https://webaudio.github.io/web-audio-api/#dom-audioworkletnode-audioworkletnode
const MIN_CHANNELS: ChannelCount = 1;
const MAX_CHANNELS: ChannelCount = 32;

// https://webaudio.github.io/web-audio-api/#supported-sample-rates
const MIN_SAMPLE_RATE: SampleRate = 3_000;
const MAX_SAMPLE_RATE: SampleRate = 768_000;

// https://webaudio.github.io/web-audio-api/#audio-processing-model
const SUPPORTED_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

// https://webaudio.github.io/web-audio-api/#render-quantum-size
const DEFAULT_RENDER_SIZE: u64 = 128;

// Must match the names passed to `registerProcessor()` in worklet.js.
const OUTPUT_PROCESSOR_NAME: &str = "CpalProcessor";
const CAPTURE_PROCESSOR_NAME: &str = "CpalCaptureProcessor";
const DUPLEX_PROCESSOR_NAME: &str = "CpalDuplexProcessor";

fn render_quantum_size_supported() -> bool {
    (|| -> Option<bool> {
        let global = js_sys::global();
        let ctor = js_sys::Reflect::get(&global, &JsValue::from("AudioContext")).ok()?;
        let proto = js_sys::Reflect::get(&ctor, &JsValue::from("prototype")).ok()?;
        js_sys::Reflect::has(&proto, &JsValue::from("renderQuantumSize")).ok()
    })()
    .unwrap_or(false)
}

fn supported_render_quantum_range(sample_rate: SampleRate) -> SupportedBufferSize {
    // https://webaudio.github.io/web-audio-api/#supported-render-quantum-sizes
    if render_quantum_size_supported() {
        SupportedBufferSize::Range {
            min: 1,
            max: sample_rate.saturating_mul(6),
        }
    } else {
        SupportedBufferSize::Range {
            min: DEFAULT_RENDER_SIZE as FrameCount,
            max: DEFAULT_RENDER_SIZE as FrameCount,
        }
    }
}

/// Checks shared by both `build_input_stream_raw` and `build_output_stream_raw`.
fn validate_config(config: &StreamConfig, sample_format: SampleFormat) -> Result<(), Error> {
    crate::validate_stream_config(config)?;
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
    if let BufferSize::Fixed(n) = config.buffer_size {
        if let SupportedBufferSize::Range { min, max } =
            supported_render_quantum_range(config.sample_rate)
        {
            if !(min..=max).contains(&n) {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!(
                        "Buffer size {n} is not in the supported render quantum range {min}..={max}"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Applies [`validate_config`] to each direction of a duplex configuration.
fn validate_duplex_config(
    config: &DuplexStreamConfig,
    input_sample_format: SampleFormat,
    output_sample_format: SampleFormat,
) -> Result<(), Error> {
    let per_direction = |channels| StreamConfig {
        channels,
        sample_rate: config.sample_rate,
        buffer_size: config.buffer_size,
    };
    validate_config(&per_direction(config.input_channels), input_sample_format)?;
    validate_config(&per_direction(config.output_channels), output_sample_format)
}

/// The full matrix of channel counts x sample rates; identical for input and output, since
/// neither is known ahead of time (see the callers' doc comments).
fn supported_configs() -> Vec<SupportedStreamConfigRange> {
    (MIN_CHANNELS..=MAX_CHANNELS)
        .flat_map(|channels| {
            crate::COMMON_SAMPLE_RATES
                .iter()
                .copied()
                .filter(|&r| (MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&r))
                .map(move |rate| SupportedStreamConfigRange {
                    channels,
                    min_sample_rate: rate,
                    max_sample_rate: rate,
                    buffer_size: supported_render_quantum_range(rate),
                    sample_format: SUPPORTED_SAMPLE_FORMAT,
                })
        })
        .collect()
}

/// Picks the best default from a supported-config iterator via the standard heuristics.
fn default_config(
    configs: impl Iterator<Item = SupportedStreamConfigRange>,
    none_supported_message: &'static str,
) -> Result<SupportedStreamConfig, Error> {
    let range = configs
        .max_by(|a, b| a.cmp_default_heuristics(b))
        .ok_or_else(|| Error::with_message(ErrorKind::UnsupportedConfig, none_supported_message))?;
    Ok(range
        .try_with_standard_sample_rate()
        .unwrap_or_else(|| range.with_max_sample_rate()))
}

enum Command {
    Play,
    Pause,
}

impl Host {
    pub fn new() -> Result<Self, Error> {
        if Self::is_available() {
            Ok(Host)
        } else {
            Err(Error::with_message(
                ErrorKind::HostUnavailable,
                "AudioWorklet is not available",
            ))
        }
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        if let Some(window) = web_sys::window() {
            let has_audio_worklet =
                js_sys::Reflect::has(&window, &JsValue::from_str("AudioWorklet")).unwrap_or(false);

            let cross_origin_isolated =
                js_sys::Reflect::get(&window, &JsValue::from_str("crossOriginIsolated"))
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

            has_audio_worklet && cross_origin_isolated
        } else {
            false
        }
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        if Self::is_available() && crate::host::is_get_user_media_available() {
            Some(Device)
        } else {
            None
        }
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        if Self::is_available() {
            Some(Device)
        } else {
            None
        }
    }
}

impl Devices {
    fn new() -> Result<Self, Error> {
        Ok(Devices(Host::is_available()))
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        let direction = if crate::host::is_get_user_media_available() {
            DeviceDirection::Duplex
        } else {
            DeviceDirection::Output
        };
        Ok(DeviceDescriptionBuilder::new("Default Device")
            .direction(direction)
            .build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId::new(
            crate::platform::HostId::AudioWorklet,
            "default",
        ))
    }

    /// One `AudioWorkletNode` renders both directions from a single `process()` call per quantum.
    fn supports_duplex(&self) -> bool {
        crate::host::is_get_user_media_available()
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        // The actual channel count and sample rate depend on the microphone getUserMedia()
        // grants access to, which isn't known ahead of time; WebAudio resamples and up/downmixes
        // whatever it gets to match, so this reports the same broad matrix as output.
        Ok(supported_configs().into_iter())
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        // In actuality the number of supported channels cannot be fully known until
        // the browser attempts to initialize the AudioWorklet.
        Ok(supported_configs().into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        default_config(
            self.supported_input_configs()?,
            "No supported input configuration",
        )
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        default_config(
            self.supported_output_configs()?,
            "No supported output configuration",
        )
    }

    /// Create an input stream capturing microphone audio via `getUserMedia()`.
    ///
    /// # Async completion
    ///
    /// This function returns `Ok` synchronously once the [`AudioContext`] is created, before
    /// microphone access has been granted or denied and before the AudioWorklet module has been
    /// loaded. Both happen asynchronously via [`wasm_bindgen_futures::spawn_local`]; if the user
    /// denies access, no microphone is present, or the worklet fails to initialize, the error is
    /// delivered to `error_callback` after the caller already holds a [`Stream`]. There is no way
    /// to surface such errors synchronously given the Web Audio API's design.
    ///
    /// [`start`](crate::traits::StreamTrait::start) and [`pause`](crate::traits::StreamTrait::pause)
    /// calls made before initialization completes return `Ok` immediately and are queued. If
    /// initialization succeeds, the queued commands take effect; if it fails they are discarded
    /// and the error is delivered to `error_callback`.
    ///
    /// [`AudioContext`]: web_sys::AudioContext
    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        validate_config(&config, sample_format)?;
        let mut data_callback = crate::host::monotonic_input_callback(data_callback);

        let n_channels = config.channels as u32;

        let stream_opts = web_sys::AudioContextOptions::new();
        stream_opts.set_sample_rate(config.sample_rate as f32);
        if let BufferSize::Fixed(n) = config.buffer_size {
            let _ = js_sys::Reflect::set(
                stream_opts.as_ref(),
                &JsValue::from_str("renderSizeHint"),
                &JsValue::from_f64(n as f64),
            );
        }

        let audio_context =
            web_sys::AudioContext::new_with_context_options(&stream_opts).map_err(|_| {
                Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    "Failed to create audio context",
                )
            })?;

        // Chrome rounds renderSizeHint to a power of two; read back the actual quantum.
        let actual_render_quantum =
            js_sys::Reflect::get(audio_context.as_ref(), &JsValue::from("renderQuantumSize"))
                .ok()
                .and_then(|v| v.as_f64())
                .map(|v| v as u64);

        let initial_quantum = actual_render_quantum.unwrap_or(match config.buffer_size {
            BufferSize::Fixed(n) => n as u64,
            BufferSize::Default => DEFAULT_RENDER_SIZE,
        });
        let buffer_size_frames = Arc::new(AtomicU64::new(initial_quantum));
        let buffer_size_frames_cb = buffer_size_frames.clone();

        let current_time_bits = Arc::new(AtomicU64::new(audio_context.current_time().to_bits()));
        let current_time_bits_cb = current_time_bits.clone();
        let current_time_bits_init = current_time_bits.clone();

        let (command_tx, mut command_rx) = mpsc::unbounded::<Command>();

        let ctx = audio_context.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let error_callback = Rc::new(RefCell::new(error_callback));

            let media_stream = match crate::host::request_microphone().await {
                Ok(stream) => stream,
                Err(js_err) => {
                    (error_callback.borrow_mut())(crate::host::get_user_media_error(&js_err));
                    let _ = audio_context.close();
                    return;
                }
            };

            let result: Result<
                (
                    web_sys::MediaStreamAudioSourceNode,
                    web_sys::AudioWorkletNode,
                    web_sys::GainNode,
                ),
                JsValue,
            > = async {
                let mod_url = dependent_module!("worklet.js")?;
                wasm_bindgen_futures::JsFuture::from(ctx.audio_worklet()?.add_module(&mod_url)?)
                    .await?;

                let source = ctx.create_media_stream_source(&media_stream)?;

                let options = web_sys::AudioWorkletNodeOptions::new();
                options.set_number_of_inputs(1);
                // A node with zero outputs and nothing downstream has no path to the
                // destination, so the graph never pulls it for processing (the same class of
                // bug as ScriptProcessorNode requiring a real output in some browsers). Give it
                // one silent output, muted below, purely to keep it in the render graph.
                options.set_number_of_outputs(1);
                options.set_output_channel_count(&js_sys::Array::of1(&JsValue::from_f64(1.0)));
                // `config.channels` is a promise to the caller: every callback delivers exactly
                // that many interleaved channels. Force WebAudio to up/downmix the microphone
                // track to match, regardless of how many channels it actually carries.
                options.set_channel_count(n_channels);
                options.set_channel_count_mode(web_sys::ChannelCountMode::Explicit);

                options.set_processor_options(Some(&js_sys::Array::of3(
                    &wasm_bindgen::module(),
                    &wasm_bindgen::memory(),
                    &WasmAudioCaptureProcessor::new(Box::new(
                        move |interleaved_data, frame_size, sample_rate, now| {
                            buffer_size_frames_cb.store(frame_size as u64, Ordering::Relaxed);
                            current_time_bits_cb.store(now.to_bits(), Ordering::Relaxed);
                            let data = interleaved_data.as_ptr() as *mut ();
                            let data = unsafe {
                                Data::from_parts(data, interleaved_data.len(), sample_format)
                            };

                            let callback = StreamInstant::from_secs_f64(now);
                            let buffer_duration =
                                frames_to_duration(frame_size as FrameCount, sample_rate);
                            let device = callback.checked_sub(buffer_duration).unwrap_or(callback);
                            let timestamp = StreamTimestamp { callback, device };
                            let info = CallbackInfo {
                                timestamp,
                                xrun: false,
                            };
                            (data_callback)(&data, &info);
                        },
                    ))
                    .pack()
                    .into(),
                )));
                let audio_worklet_node = web_sys::AudioWorkletNode::new_with_options(
                    &ctx,
                    CAPTURE_PROCESSOR_NAME,
                    &options,
                )?;

                let error_callback_setup = error_callback.clone();
                let on_processor_error =
                    Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
                        (error_callback_setup.borrow_mut())(Error::with_message(
                            ErrorKind::BackendError,
                            "AudioWorklet capture processor failed to initialize or crashed",
                        ));
                    });
                audio_worklet_node
                    .set_onprocessorerror(Some(on_processor_error.as_ref().unchecked_ref()));
                on_processor_error.forget();

                source.connect_with_audio_node(&audio_worklet_node)?;

                // Route the node's silent dummy output through a muted gain to the
                // destination, keeping it in the render graph without producing audible sound.
                let mute_gain = web_sys::GainNode::new(&ctx)?;
                mute_gain.gain().set_value(0.0);
                audio_worklet_node.connect_with_audio_node(&mute_gain)?;
                mute_gain.connect_with_audio_node(&ctx.destination())?;

                Ok((source, audio_worklet_node, mute_gain))
            }
            .await;

            // Unlike AudioWorkletNode/GainNode, a MediaStreamAudioSourceNode isn't an
            // AudioScheduledSourceNode with a spec-guaranteed lifetime tied to the render graph:
            // browsers may garbage-collect it once its last JS reference is dropped, silently
            // killing the mic connection a few render quanta later. Keep all three alive for as
            // long as the command loop below runs, i.e. until the Stream is dropped.
            let (_source, _audio_worklet_node, _mute_gain) = match result {
                Ok(nodes) => nodes,
                Err(err) => {
                    let message = err
                        .as_string()
                        .unwrap_or_else(|| "Failed to initialize audio worklet".to_string());
                    (error_callback.borrow_mut())(Error::with_message(
                        ErrorKind::HostUnavailable,
                        message,
                    ));

                    crate::host::stop_tracks(&media_stream);
                    let _ = audio_context.close();
                    return;
                }
            };

            current_time_bits_init.store(audio_context.current_time().to_bits(), Ordering::Relaxed);

            // Process play/pause commands from any thread until Stream is dropped.
            // Dropping Stream closes command_tx, which terminates this loop.
            while let Some(cmd) = command_rx.next().await {
                match cmd {
                    Command::Play => {
                        if audio_context.resume().is_err() {
                            (error_callback.borrow_mut())(Error::with_message(
                                ErrorKind::DeviceNotAvailable,
                                "Failed to resume audio context",
                            ));
                        }
                    }
                    Command::Pause => {
                        if audio_context.suspend().is_err() {
                            (error_callback.borrow_mut())(Error::with_message(
                                ErrorKind::DeviceNotAvailable,
                                "Failed to suspend audio context",
                            ));
                        }
                    }
                }
            }

            // Stream dropped: release the microphone and close the AudioContext.
            crate::host::stop_tracks(&media_stream);
            let _ = audio_context.close();
        });

        Ok(Self::Stream {
            command_tx,
            current_time_bits,
            buffer_size_frames,
        })
    }

    /// Create an output stream.
    ///
    /// # Async completion
    ///
    /// This function returns `Ok` synchronously once the [`AudioContext`] is created, before the
    /// AudioWorklet module has been loaded or the [`AudioWorkletNode`] has been initialized. The
    /// actual worklet setup runs asynchronously via [`wasm_bindgen_futures::spawn_local`]. If
    /// setup fails (e.g. `add_module` or `AudioWorkletNode` construction throws), the error is
    /// delivered to `error_callback` after the caller already holds a [`Stream`]. There is no
    /// way to surface such errors synchronously given the Web Audio API's design.
    ///
    /// [`start`](crate::traits::StreamTrait::start) and [`pause`](crate::traits::StreamTrait::pause)
    /// calls made before initialization completes return `Ok` immediately and are queued. If
    /// initialization succeeds, then the queued commands take effect; if it fails they are
    /// discarded and the error is delivered to `error_callback`.
    ///
    /// [`now`](crate::traits::StreamTrait::now) returns the scheduled time of the last rendered
    /// audio frame, seeded from [`AudioContext::current_time`] at construction. While the stream
    /// is paused, this value does not advance. This is consistent with the [`AudioContext`] clock,
    /// which also freezes when paused.
    ///
    /// [`AudioContext`]: web_sys::AudioContext
    /// [`AudioContext::current_time`]: https://developer.mozilla.org/en-US/docs/Web/API/BaseAudioContext/currentTime
    /// [`AudioWorkletNode`]: web_sys::AudioWorkletNode
    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        validate_config(&config, sample_format)?;
        // Keep `device` monotonic: the polled outputLatency can drop when the
        // page calls `setSinkId()` to switch output devices, pulling `device` backward.
        let mut data_callback = crate::host::monotonic_output_callback(data_callback);

        let stream_opts = web_sys::AudioContextOptions::new();
        stream_opts.set_sample_rate(config.sample_rate as f32);
        if let BufferSize::Fixed(n) = config.buffer_size {
            let _ = js_sys::Reflect::set(
                stream_opts.as_ref(),
                &JsValue::from_str("renderSizeHint"),
                &JsValue::from_f64(n as f64),
            );
        }

        let audio_context =
            web_sys::AudioContext::new_with_context_options(&stream_opts).map_err(|_| {
                Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    "Failed to create audio context",
                )
            })?;

        let destination = audio_context.destination();

        // Chrome rounds renderSizeHint to a power of two; read back the actual quantum.
        let actual_render_quantum =
            js_sys::Reflect::get(audio_context.as_ref(), &JsValue::from("renderQuantumSize"))
                .ok()
                .and_then(|v| v.as_f64())
                .map(|v| v as u64);

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

        let initial_quantum = actual_render_quantum.unwrap_or(match config.buffer_size {
            BufferSize::Fixed(n) => n as u64,
            BufferSize::Default => DEFAULT_RENDER_SIZE,
        });
        let buffer_size_frames = Arc::new(AtomicU64::new(initial_quantum));
        let buffer_size_frames_cb = buffer_size_frames.clone();

        let current_time_bits = Arc::new(AtomicU64::new(audio_context.current_time().to_bits()));
        let current_time_bits_cb = current_time_bits.clone();
        let current_time_bits_init = current_time_bits.clone();

        let (command_tx, mut command_rx) = mpsc::unbounded::<Command>();

        let latency_nanos = Arc::new(AtomicU64::new(total_latency_nanos(&audio_context)));
        let latency_nanos_cb = latency_nanos.clone();

        let ctx = audio_context.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let error_callback = Rc::new(RefCell::new(error_callback));
            let error_callback_setup = error_callback.clone();

            let result: Result<(), JsValue> = async move {
                let mod_url = dependent_module!("worklet.js")?;
                wasm_bindgen_futures::JsFuture::from(ctx.audio_worklet()?.add_module(&mod_url)?)
                    .await?;

                let options = web_sys::AudioWorkletNodeOptions::new();

                let js_array = js_sys::Array::new();
                js_array.push(&JsValue::from_f64(destination.channel_count() as _));

                options.set_output_channel_count(&js_array);
                options.set_number_of_inputs(0);

                options.set_processor_options(Some(&js_sys::Array::of3(
                    &wasm_bindgen::module(),
                    &wasm_bindgen::memory(),
                    &WasmAudioProcessor::new(Box::new(
                        move |interleaved_data, frame_size, sample_rate, now| {
                            buffer_size_frames_cb.store(frame_size as u64, Ordering::Relaxed);
                            current_time_bits_cb.store(now.to_bits(), Ordering::Relaxed);
                            let data = interleaved_data.as_mut_ptr() as *mut ();
                            let mut data = unsafe {
                                Data::from_parts(data, interleaved_data.len(), sample_format)
                            };

                            let callback = StreamInstant::from_secs_f64(now);
                            let buffer_duration =
                                frames_to_duration(frame_size as FrameCount, sample_rate);
                            let latency =
                                Duration::from_nanos(latency_nanos_cb.load(Ordering::Relaxed));
                            let device = callback + (buffer_duration + latency);
                            let timestamp = StreamTimestamp { callback, device };
                            let info = CallbackInfo {
                                timestamp,
                                xrun: false,
                            };
                            (data_callback)(&mut data, &info);
                        },
                    ))
                    .pack()
                    .into(),
                )));
                let audio_worklet_node = web_sys::AudioWorkletNode::new_with_options(
                    &ctx,
                    OUTPUT_PROCESSOR_NAME,
                    &options,
                )?;

                let on_processor_error =
                    Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
                        (error_callback_setup.borrow_mut())(Error::with_message(
                            ErrorKind::BackendError,
                            "AudioWorklet processor failed to initialize or crashed",
                        ));
                    });
                audio_worklet_node
                    .set_onprocessorerror(Some(on_processor_error.as_ref().unchecked_ref()));
                on_processor_error.forget();

                audio_worklet_node.connect_with_audio_node(&destination)?;
                Ok(())
            }
            .await;

            if let Err(err) = result {
                let message = err
                    .as_string()
                    .unwrap_or_else(|| "Failed to initialize audio worklet".to_string());
                (error_callback.borrow_mut())(Error::with_message(
                    ErrorKind::HostUnavailable,
                    message,
                ));

                // Close AudioContext and exit; dropping command_rx closes the channel,
                // so subsequent play()/pause() calls return HostUnavailable.
                let _ = audio_context.close();
                return;
            }

            current_time_bits_init.store(audio_context.current_time().to_bits(), Ordering::Relaxed);

            // outputLatency can change at runtime (e.g. an output-device switch) but is only
            // readable on the main thread, so poll it here and publish it to the worklet via the
            // shared atomic.
            let _latency_poller = web_sys::window().and_then(|window| {
                let poll_ctx = audio_context.clone();
                let poll_latency = latency_nanos.clone();
                let closure = Closure::<dyn FnMut()>::new(move || {
                    poll_latency.store(total_latency_nanos(&poll_ctx), Ordering::Relaxed);
                });
                window
                    .set_interval_with_callback_and_timeout_and_arguments_0(
                        closure.as_ref().unchecked_ref(),
                        LATENCY_POLL_INTERVAL.as_millis() as i32,
                    )
                    .ok()
                    .map(|interval_id| LatencyPoller {
                        window,
                        interval_id,
                        _closure: closure,
                    })
            });

            // Process play/pause commands from any thread until Stream is dropped.
            // Dropping Stream closes command_tx, which terminates this loop.
            while let Some(cmd) = command_rx.next().await {
                match cmd {
                    Command::Play => {
                        if audio_context.resume().is_err() {
                            (error_callback.borrow_mut())(Error::with_message(
                                ErrorKind::DeviceNotAvailable,
                                "Failed to resume audio context",
                            ));
                        }
                    }
                    Command::Pause => {
                        if audio_context.suspend().is_err() {
                            (error_callback.borrow_mut())(Error::with_message(
                                ErrorKind::DeviceNotAvailable,
                                "Failed to suspend audio context",
                            ));
                        }
                    }
                }
            }

            // Stream dropped: close the AudioContext on the main thread.
            let _ = audio_context.close();
        });

        Ok(Self::Stream {
            command_tx,
            current_time_bits,
            buffer_size_frames,
        })
    }

    /// Create a duplex stream.
    ///
    /// # Async completion
    ///
    /// Behaves like [`build_input_stream_raw`](Self::build_input_stream_raw): this returns `Ok`
    /// once the [`AudioContext`] exists, before microphone permission has been resolved and
    /// before the worklet module has loaded. Failures after that point are delivered to
    /// `error_callback`, and [`start`](crate::traits::StreamTrait::start) /
    /// [`pause`](crate::traits::StreamTrait::pause) calls made in the meantime are queued.
    ///
    /// [`AudioContext`]: web_sys::AudioContext
    fn build_duplex_stream_raw<D, E>(
        &self,
        config: DuplexStreamConfig,
        input_sample_format: SampleFormat,
        output_sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        validate_duplex_config(&config, input_sample_format, output_sample_format)?;
        // Keep both `device` timestamps monotonic: the polled outputLatency can drop when the
        // page calls `setSinkId()` to switch output devices, pulling `device` backward.
        let mut data_callback = crate::host::monotonic_duplex_callback(data_callback);

        let input_channels = config.input_channels as u32;
        let output_channels = config.output_channels as u32;

        let stream_opts = web_sys::AudioContextOptions::new();
        stream_opts.set_sample_rate(config.sample_rate as f32);
        if let BufferSize::Fixed(n) = config.buffer_size {
            let _ = js_sys::Reflect::set(
                stream_opts.as_ref(),
                &JsValue::from_str("renderSizeHint"),
                &JsValue::from_f64(n as f64),
            );
        }

        let audio_context =
            web_sys::AudioContext::new_with_context_options(&stream_opts).map_err(|_| {
                Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    "Failed to create audio context",
                )
            })?;

        let destination = audio_context.destination();
        if output_channels > destination.max_channel_count() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "Output channel count {} exceeds the destination's maximum of {}",
                    config.output_channels,
                    destination.max_channel_count()
                ),
            ));
        }
        destination.set_channel_count(output_channels);

        // Chrome rounds renderSizeHint to a power of two; read back the actual quantum.
        let actual_render_quantum =
            js_sys::Reflect::get(audio_context.as_ref(), &JsValue::from("renderQuantumSize"))
                .ok()
                .and_then(|v| v.as_f64())
                .map(|v| v as u64);

        let initial_quantum = actual_render_quantum.unwrap_or(match config.buffer_size {
            BufferSize::Fixed(n) => n as u64,
            BufferSize::Default => DEFAULT_RENDER_SIZE,
        });
        let buffer_size_frames = Arc::new(AtomicU64::new(initial_quantum));
        let buffer_size_frames_cb = buffer_size_frames.clone();

        let current_time_bits = Arc::new(AtomicU64::new(audio_context.current_time().to_bits()));
        let current_time_bits_cb = current_time_bits.clone();
        let current_time_bits_init = current_time_bits.clone();

        let latency_nanos = Arc::new(AtomicU64::new(total_latency_nanos(&audio_context)));
        let latency_nanos_cb = latency_nanos.clone();

        let (command_tx, mut command_rx) = mpsc::unbounded::<Command>();

        let ctx = audio_context.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let error_callback = Rc::new(RefCell::new(error_callback));

            let media_stream = match crate::host::request_microphone().await {
                Ok(stream) => stream,
                Err(js_err) => {
                    (error_callback.borrow_mut())(crate::host::get_user_media_error(&js_err));
                    let _ = audio_context.close();
                    return;
                }
            };

            let result: Result<
                (
                    web_sys::MediaStreamAudioSourceNode,
                    web_sys::AudioWorkletNode,
                ),
                JsValue,
            > = async {
                let mod_url = dependent_module!("worklet.js")?;
                wasm_bindgen_futures::JsFuture::from(ctx.audio_worklet()?.add_module(&mod_url)?)
                    .await?;

                let source = ctx.create_media_stream_source(&media_stream)?;

                let options = web_sys::AudioWorkletNodeOptions::new();
                options.set_number_of_inputs(1);
                options.set_number_of_outputs(1);
                options.set_output_channel_count(&js_sys::Array::of1(&JsValue::from_f64(
                    output_channels as f64,
                )));
                // `config.input_channels` is a promise to the caller: every callback delivers
                // exactly that many interleaved channels. Force WebAudio to up/downmix the
                // microphone track to match, regardless of how many channels it actually carries.
                options.set_channel_count(input_channels);
                options.set_channel_count_mode(web_sys::ChannelCountMode::Explicit);

                options.set_processor_options(Some(&js_sys::Array::of3(
                    &wasm_bindgen::module(),
                    &wasm_bindgen::memory(),
                    &WasmAudioDuplexProcessor::new(Box::new(
                        move |input_interleaved,
                              output_interleaved,
                              frame_size,
                              sample_rate,
                              now| {
                            buffer_size_frames_cb.store(frame_size as u64, Ordering::Relaxed);
                            current_time_bits_cb.store(now.to_bits(), Ordering::Relaxed);

                            let input = unsafe {
                                Data::from_parts(
                                    input_interleaved.as_ptr() as *mut (),
                                    input_interleaved.len(),
                                    input_sample_format,
                                )
                            };
                            let mut output = unsafe {
                                Data::from_parts(
                                    output_interleaved.as_mut_ptr() as *mut (),
                                    output_interleaved.len(),
                                    output_sample_format,
                                )
                            };

                            // One clock: both directions share the same `callback` instant.
                            let callback = StreamInstant::from_secs_f64(now);
                            let buffer_duration =
                                frames_to_duration(frame_size as FrameCount, sample_rate);
                            let latency =
                                Duration::from_nanos(latency_nanos_cb.load(Ordering::Relaxed));

                            let info = DuplexCallbackInfo::new(
                                CallbackInfo {
                                    timestamp: StreamTimestamp {
                                        callback,
                                        device: callback
                                            .checked_sub(buffer_duration)
                                            .unwrap_or(callback),
                                    },
                                    xrun: false,
                                },
                                CallbackInfo {
                                    timestamp: StreamTimestamp {
                                        callback,
                                        device: callback + (buffer_duration + latency),
                                    },
                                    xrun: false,
                                },
                            );
                            (data_callback)(&input, &mut output, &info);
                        },
                    ))
                    .pack()
                    .into(),
                )));
                let audio_worklet_node = web_sys::AudioWorkletNode::new_with_options(
                    &ctx,
                    DUPLEX_PROCESSOR_NAME,
                    &options,
                )?;

                let error_callback_setup = error_callback.clone();
                let on_processor_error =
                    Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
                        (error_callback_setup.borrow_mut())(Error::with_message(
                            ErrorKind::BackendError,
                            "AudioWorklet duplex processor failed to initialize or crashed",
                        ));
                    });
                audio_worklet_node
                    .set_onprocessorerror(Some(on_processor_error.as_ref().unchecked_ref()));
                on_processor_error.forget();

                // Unlike the capture-only path, the node's output is the real playback signal,
                // so it goes straight to the destination rather than through a muted gain.
                source.connect_with_audio_node(&audio_worklet_node)?;
                audio_worklet_node.connect_with_audio_node(&destination)?;

                Ok((source, audio_worklet_node))
            }
            .await;

            // Keep both alive until the Stream is dropped, or the source may be garbage-collected
            // and silently kill the mic connection; see `build_input_stream_raw`.
            let (_source, _audio_worklet_node) = match result {
                Ok(nodes) => nodes,
                Err(err) => {
                    let message = err
                        .as_string()
                        .unwrap_or_else(|| "Failed to initialize audio worklet".to_string());
                    (error_callback.borrow_mut())(Error::with_message(
                        ErrorKind::HostUnavailable,
                        message,
                    ));

                    crate::host::stop_tracks(&media_stream);
                    let _ = audio_context.close();
                    return;
                }
            };

            current_time_bits_init.store(audio_context.current_time().to_bits(), Ordering::Relaxed);

            // outputLatency can change at runtime (e.g. an output-device switch) but is only
            // readable on the main thread, so poll it here and publish it to the worklet via the
            // shared atomic.
            let _latency_poller = web_sys::window().and_then(|window| {
                let poll_ctx = audio_context.clone();
                let poll_latency = latency_nanos.clone();
                let closure = Closure::<dyn FnMut()>::new(move || {
                    poll_latency.store(total_latency_nanos(&poll_ctx), Ordering::Relaxed);
                });
                window
                    .set_interval_with_callback_and_timeout_and_arguments_0(
                        closure.as_ref().unchecked_ref(),
                        LATENCY_POLL_INTERVAL.as_millis() as i32,
                    )
                    .ok()
                    .map(|interval_id| LatencyPoller {
                        window,
                        interval_id,
                        _closure: closure,
                    })
            });

            // Process play/pause commands from any thread until Stream is dropped.
            // Dropping Stream closes command_tx, which terminates this loop.
            while let Some(cmd) = command_rx.next().await {
                match cmd {
                    Command::Play => {
                        if audio_context.resume().is_err() {
                            (error_callback.borrow_mut())(Error::with_message(
                                ErrorKind::DeviceNotAvailable,
                                "Failed to resume audio context",
                            ));
                        }
                    }
                    Command::Pause => {
                        if audio_context.suspend().is_err() {
                            (error_callback.borrow_mut())(Error::with_message(
                                ErrorKind::DeviceNotAvailable,
                                "Failed to suspend audio context",
                            ));
                        }
                    }
                }
            }

            // Stream dropped: release the microphone and close the AudioContext.
            crate::host::stop_tracks(&media_stream);
            let _ = audio_context.close();
        });

        Ok(Self::Stream {
            command_tx,
            current_time_bits,
            buffer_size_frames,
        })
    }
}

impl StreamTrait for Stream {
    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.buffer_size_frames.load(Ordering::Relaxed) as FrameCount)
    }

    fn start(&self) -> Result<(), Error> {
        self.command_tx.unbounded_send(Command::Play).map_err(|_| {
            Error::with_message(
                ErrorKind::HostUnavailable,
                "audio worklet initialization failed",
            )
        })
    }

    fn pause(&self) -> Result<(), Error> {
        self.command_tx.unbounded_send(Command::Pause).map_err(|_| {
            Error::with_message(
                ErrorKind::HostUnavailable,
                "audio worklet initialization failed",
            )
        })
    }

    fn stop(&self, _timeout: Option<std::time::Duration>) -> Result<(), Error> {
        self.pause()
    }

    fn now(&self) -> StreamInstant {
        StreamInstant::from_secs_f64(f64::from_bits(
            self.current_time_bits.load(Ordering::Relaxed),
        ))
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

/// Grows `buffer` so it can hold `channels * frame_size` interleaved samples, never shrinking it,
/// and returns the length of the region actually in use.
fn resize_interleaved(buffer: &mut Vec<f32>, channels: u32, frame_size: u32) -> usize {
    let len = channels as usize * frame_size as usize;
    buffer.resize(len.max(buffer.len()), f32::EQUILIBRIUM);
    len
}

// The interleaved buffer, plus frame size, sample rate, and current time.
type AudioProcessorCallback = Box<dyn FnMut(&mut [f32], u32, u32, f64)>;

/// WasmAudioProcessor provides an interface for the JavaScript code
/// running in the AudioWorklet to interact with Rust.
#[wasm_bindgen]
pub struct WasmAudioProcessor {
    interleaved_buffer: Vec<f32>,
    // Passes in an interleaved scratch buffer, frame size, sample rate, and current time.
    callback: AudioProcessorCallback,
}

impl WasmAudioProcessor {
    pub fn new(callback: AudioProcessorCallback) -> Self {
        Self {
            interleaved_buffer: Vec::new(),
            callback,
        }
    }
}

#[wasm_bindgen]
impl WasmAudioProcessor {
    pub fn process(
        &mut self,
        channels: u32,
        frame_size: u32,
        sample_rate: u32,
        current_time: f64,
    ) -> u32 {
        let interleaved_buffer_size =
            resize_interleaved(&mut self.interleaved_buffer, channels, frame_size);
        self.interleaved_buffer[..interleaved_buffer_size].fill(f32::EQUILIBRIUM);

        (self.callback)(
            &mut self.interleaved_buffer[..interleaved_buffer_size],
            frame_size,
            sample_rate,
            current_time,
        );

        // Returns a pointer to the raw interleaved buffer to Javascript so
        // it can deinterleave it into the output buffers.
        self.interleaved_buffer.as_mut_ptr() as _
    }

    /// Converts this `WasmAudioProcessor` into a raw pointer (as `usize`) for FFI use.
    ///
    /// Transfers ownership of the processor to the caller. The returned pointer must be passed to
    /// [`unpack`] exactly once. Failing to call [`unpack`] will leak the allocation.
    ///
    /// [`unpack`]: Self::unpack
    pub fn pack(self) -> usize {
        Box::into_raw(Box::new(self)) as usize
    }
    /// # Safety
    ///
    /// The `val` parameter must be a value previously returned by `Self::pack`.
    /// It must not have already been unpacked or deallocated, and must not be used after this call.
    /// Using an invalid or already-consumed pointer will result in undefined behavior.
    pub unsafe fn unpack(val: usize) -> Self {
        unsafe { *Box::from_raw(val as *mut _) }
    }
}

type AudioCaptureCallback = Box<dyn FnMut(&[f32], u32, u32, f64)>;

/// WasmAudioCaptureProcessor provides an interface for the JavaScript code running in the
/// AudioWorklet to hand captured microphone audio to Rust. The mirror image of
/// [`WasmAudioProcessor`]: JS interleaves the input channels into a Rust-owned buffer instead of
/// Rust filling a buffer for JS to deinterleave out.
#[wasm_bindgen]
pub struct WasmAudioCaptureProcessor {
    interleaved_buffer: Vec<f32>,
    // Receives the interleaved captured buffer, frame size, sample rate, and current time.
    callback: AudioCaptureCallback,
}

impl WasmAudioCaptureProcessor {
    pub fn new(callback: AudioCaptureCallback) -> Self {
        Self {
            interleaved_buffer: Vec::new(),
            callback,
        }
    }
}

#[wasm_bindgen]
impl WasmAudioCaptureProcessor {
    /// Ensures the capture buffer can hold `channels * frame_size` samples and returns a pointer
    /// for JS to interleave captured audio into.
    pub fn capture_buffer_ptr(&mut self, channels: u32, frame_size: u32) -> u32 {
        resize_interleaved(&mut self.interleaved_buffer, channels, frame_size);
        self.interleaved_buffer.as_mut_ptr() as _
    }

    /// Invokes the Rust callback with the interleaved audio JS wrote via the pointer returned by
    /// [`capture_buffer_ptr`](Self::capture_buffer_ptr).
    pub fn process_captured(
        &mut self,
        channels: u32,
        frame_size: u32,
        sample_rate: u32,
        current_time: f64,
    ) {
        let interleaved_buffer_size = channels as usize * frame_size as usize;
        (self.callback)(
            &self.interleaved_buffer[..interleaved_buffer_size],
            frame_size,
            sample_rate,
            current_time,
        );
    }

    pub fn pack(self) -> usize {
        Box::into_raw(Box::new(self)) as usize
    }
    /// # Safety
    ///
    /// The `val` parameter must be a value previously returned by `Self::pack`.
    /// It must not have already been unpacked or deallocated, and must not be used after this call.
    /// Using an invalid or already-consumed pointer will result in undefined behavior.
    pub unsafe fn unpack(val: usize) -> Self {
        unsafe { *Box::from_raw(val as *mut _) }
    }
}

// The interleaved captured buffer and an interleaved buffer to render into, plus frame size,
// sample rate, and current time.
type AudioDuplexCallback = Box<dyn FnMut(&[f32], &mut [f32], u32, u32, f64)>;

/// WasmAudioDuplexProcessor provides an interface for the JavaScript code running in the
/// AudioWorklet to hand captured audio to Rust and take rendered audio back in one call.
#[wasm_bindgen]
pub struct WasmAudioDuplexProcessor {
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    callback: AudioDuplexCallback,
}

impl WasmAudioDuplexProcessor {
    pub fn new(callback: AudioDuplexCallback) -> Self {
        Self {
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
            callback,
        }
    }
}

#[wasm_bindgen]
impl WasmAudioDuplexProcessor {
    /// Sizes both buffers for this quantum and returns a pointer for JS to interleave captured
    /// audio into. Must be called to grow Wasm memory before [`process`](Self::process) runs.
    pub fn prepare(&mut self, input_channels: u32, output_channels: u32, frame_size: u32) -> u32 {
        resize_interleaved(&mut self.input_buffer, input_channels, frame_size);
        resize_interleaved(&mut self.output_buffer, output_channels, frame_size);
        self.input_buffer.as_mut_ptr() as _
    }

    /// Pointer for JS to deinterleave out. Valid only once [`prepare`](Self::prepare) has run.
    pub fn output_buffer_ptr(&mut self) -> u32 {
        self.output_buffer.as_mut_ptr() as _
    }

    /// Invokes the Rust callback with the captured audio, plus the output buffer to fill.
    pub fn process(
        &mut self,
        input_channels: u32,
        output_channels: u32,
        frame_size: u32,
        sample_rate: u32,
        current_time: f64,
    ) {
        let input_len = input_channels as usize * frame_size as usize;
        let output_len = output_channels as usize * frame_size as usize;

        // Destructured so the callback can hold both buffers at once.
        let Self {
            input_buffer,
            output_buffer,
            callback,
        } = self;
        output_buffer[..output_len].fill(f32::EQUILIBRIUM);
        callback(
            &input_buffer[..input_len],
            &mut output_buffer[..output_len],
            frame_size,
            sample_rate,
            current_time,
        );
    }

    pub fn pack(self) -> usize {
        Box::into_raw(Box::new(self)) as usize
    }

    /// # Safety
    ///
    /// The `val` parameter must be a value previously returned by `Self::pack`.
    /// It must not have already been unpacked or deallocated, and must not be used after this call.
    /// Using an invalid or already-consumed pointer will result in undefined behavior.
    pub unsafe fn unpack(val: usize) -> Self {
        unsafe { *Box::from_raw(val as *mut _) }
    }
}

/// Drives a `setInterval` that refreshes the shared output-latency value.
struct LatencyPoller {
    window: web_sys::Window,
    interval_id: i32,
    _closure: Closure<dyn FnMut()>,
}

impl Drop for LatencyPoller {
    fn drop(&mut self) {
        self.window.clear_interval_with_handle(self.interval_id);
    }
}

/// Reads the playback buffer depth from a context.
fn total_latency_nanos(ctx: &web_sys::AudioContext) -> u64 {
    let read = |key: &str| {
        js_sys::Reflect::get(ctx.as_ref(), &JsValue::from(key))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };
    // `baseLatency` is fixed for the context lifetime; `outputLatency` can change.
    let secs = read("baseLatency") + read("outputLatency");
    if secs.is_finite() && secs > 0.0 {
        (secs * 1_000_000_000.0).round() as u64
    } else {
        0
    }
}
