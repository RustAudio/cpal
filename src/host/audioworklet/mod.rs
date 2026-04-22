//! Audio Worklet backend implementation.
//!
//! Available on WebAssembly with the `audioworklet` feature. Requires atomics support.
//! See the `audioworklet-beep` example for setup instructions.

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use js_sys::wasm_bindgen;
use wasm_bindgen::prelude::*;

use crate::{
    host::frames_to_duration,
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceDirection,
    DeviceId, Error, ErrorKind, FrameCount, InputCallbackInfo, OutputCallbackInfo,
    OutputStreamTimestamp, SampleFormat, SampleRate, StreamConfig, StreamInstant,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
};

mod dependent_module;
use crate::dependent_module;

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Device;

pub struct Host;

pub struct Stream {
    audio_context: web_sys::AudioContext,
    buffer_size_frames: Arc<AtomicU64>,
}

pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

const MIN_CHANNELS: ChannelCount = 1;
const MAX_CHANNELS: ChannelCount = 32;
const MIN_SAMPLE_RATE: SampleRate = 8_000;
const MAX_SAMPLE_RATE: SampleRate = 96_000;
const DEFAULT_SAMPLE_RATE: SampleRate = 44_100;
const SUPPORTED_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

// https://webaudio.github.io/web-audio-api/#render-quantum-size
const DEFAULT_RENDER_SIZE: u64 = 128;

impl Host {
    pub fn new() -> Result<Self, Error> {
        if Self::is_available() {
            Ok(Host)
        } else {
            Err(Error::with_message(
                ErrorKind::HostUnavailable,
                "AudioWorklet API is not available",
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
        // TODO
        None
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        Some(Device)
    }
}

impl Devices {
    fn new() -> Result<Self, Error> {
        Ok(Self::default())
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Ok(DeviceDescriptionBuilder::new("Default Device".to_string())
            .direction(DeviceDirection::Output)
            .build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId(
            crate::platform::HostId::AudioWorklet,
            "default".to_string(),
        ))
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        // TODO
        Ok(Vec::new().into_iter())
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        let buffer_size = SupportedBufferSize::Unknown;

        // In actuality the number of supported channels cannot be fully known until
        // the browser attempts to initialized the AudioWorklet.

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
            "AudioWorklet does not support audio input",
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
            "AudioWorklet does not support audio input",
        ))
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
    /// [`AudioContext`]: web_sys::AudioContext
    /// [`AudioWorkletNode`]: web_sys::AudioWorkletNode
    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        if config.channels < MIN_CHANNELS || config.channels > MAX_CHANNELS {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "{} channels is not supported; AudioWorklet supports {} to {}",
                    config.channels, MIN_CHANNELS, MAX_CHANNELS
                ),
            ));
        }
        if config.sample_rate < MIN_SAMPLE_RATE || config.sample_rate > MAX_SAMPLE_RATE {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "{} Hz is not supported; AudioWorklet supports {} to {} Hz",
                    config.sample_rate, MIN_SAMPLE_RATE, MAX_SAMPLE_RATE
                ),
            ));
        }
        if sample_format != SUPPORTED_SAMPLE_FORMAT {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "sample format {sample_format} is not supported; AudioWorklet requires {SUPPORTED_SAMPLE_FORMAT}"
                ),
            ));
        }

        let stream_opts = web_sys::AudioContextOptions::new();
        stream_opts.set_sample_rate(config.sample_rate as f32);
        if let BufferSize::Fixed(n) = config.buffer_size {
            let _ = js_sys::Reflect::set(
                stream_opts.as_ref(),
                &JsValue::from_str("renderSizeHint"),
                &JsValue::from_f64(n as f64),
            );
        }

        let audio_context = web_sys::AudioContext::new_with_context_options(&stream_opts)
            .map_err(|err| Error::with_message(ErrorKind::UnsupportedConfig, format!("{err:?}")))?;

        let destination = audio_context.destination();

        // If possible, set the destination's channel_count to the given config.channel.
        // If not, fallback on the default destination channel_count to keep previous behavior
        // and do not return an error.
        if config.channels as u32 <= destination.max_channel_count() {
            destination.set_channel_count(config.channels as u32);
        }

        let initial_quantum = match config.buffer_size {
            BufferSize::Fixed(n) => n as u64,
            BufferSize::Default => DEFAULT_RENDER_SIZE,
        };
        let buffer_size_frames = Arc::new(AtomicU64::new(initial_quantum));
        let buffer_size_frames_cb = buffer_size_frames.clone();
        let ctx = audio_context.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result: Result<(), JsValue> = async move {
                let mod_url = dependent_module!("worklet.js")?;
                wasm_bindgen_futures::JsFuture::from(ctx.audio_worklet()?.add_module(&mod_url)?)
                    .await?;

                let options = web_sys::AudioWorkletNodeOptions::new();

                let js_array = js_sys::Array::new();
                js_array.push(&JsValue::from_f64(destination.channel_count() as _));

                options.set_output_channel_count(&js_array);
                options.set_number_of_inputs(0);

                // Capture audio output latency here: the closure runs in a separate worker and cannot access AudioContext properties directly.
                // While baseLatency is fixed for the context lifetime, outputLatency can change but not be re-read from inside the worklet;
                // we snapshot it here.
                let base_latency_secs =
                    js_sys::Reflect::get(ctx.as_ref(), &JsValue::from("baseLatency"))
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                let output_latency_secs =
                    js_sys::Reflect::get(ctx.as_ref(), &JsValue::from("outputLatency"))
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                let total_output_latency_secs = {
                    let sum = base_latency_secs + output_latency_secs;
                    if sum.is_finite() {
                        sum.max(0.0)
                    } else {
                        0.0
                    }
                };

                options.set_processor_options(Some(&js_sys::Array::of3(
                    &wasm_bindgen::module(),
                    &wasm_bindgen::memory(),
                    &WasmAudioProcessor::new(Box::new(
                        move |interleaved_data, frame_size, sample_rate, now| {
                            buffer_size_frames_cb.store(frame_size as u64, Ordering::Relaxed);
                            let data = interleaved_data.as_mut_ptr() as *mut ();
                            let mut data = unsafe {
                                Data::from_parts(data, interleaved_data.len(), sample_format)
                            };

                            let callback = StreamInstant::from_secs_f64(now);
                            let buffer_duration =
                                frames_to_duration(frame_size as FrameCount, sample_rate);
                            let playback = callback
                                + (buffer_duration
                                    + Duration::from_secs_f64(total_output_latency_secs));
                            let timestamp = OutputStreamTimestamp { callback, playback };
                            let info = OutputCallbackInfo { timestamp };
                            (data_callback)(&mut data, &info);
                        },
                    ))
                    .pack()
                    .into(),
                )));
                // This name 'CpalProcessor' must match the name registered in worklet.js
                let audio_worklet_node =
                    web_sys::AudioWorkletNode::new_with_options(&ctx, "CpalProcessor", &options)?;

                audio_worklet_node.connect_with_audio_node(&destination)?;
                Ok(())
            }
            .await;

            if let Err(err) = result {
                let message = err
                    .as_string()
                    .unwrap_or_else(|| format!("Browser error initializing stream: {err:?}"));
                error_callback(Error::with_message(
                    ErrorKind::UnsupportedOperation,
                    message,
                ))
            }
        });

        Ok(Self::Stream {
            audio_context,
            buffer_size_frames,
        })
    }
}

impl StreamTrait for Stream {
    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.buffer_size_frames.load(Ordering::Relaxed) as FrameCount)
    }

    fn play(&self) -> Result<(), Error> {
        match self.audio_context.resume() {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                format!("{err:?}"),
            )),
        }
    }

    fn pause(&self) -> Result<(), Error> {
        match self.audio_context.suspend() {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                format!("{err:?}"),
            )),
        }
    }

    fn now(&self) -> StreamInstant {
        StreamInstant::from_secs_f64(self.audio_context.current_time())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.audio_context.close();
    }
}

impl Default for Devices {
    fn default() -> Self {
        Self(true)
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

type AudioProcessorCallback = Box<dyn FnMut(&mut [f32], u32, u32, f64)>;

/// WasmAudioProcessor provides an interface for the Javascript code
/// running in the AudioWorklet to interact with Rust.
#[wasm_bindgen]
pub struct WasmAudioProcessor {
    #[wasm_bindgen(skip)]
    interleaved_buffer: Vec<f32>,
    #[wasm_bindgen(skip)]
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
        let frame_size = frame_size as usize;

        // Ensure there's enough space in the output buffer
        // This likely only occurs once, or very few times.
        let interleaved_buffer_size = channels as usize * frame_size;
        self.interleaved_buffer.resize(
            interleaved_buffer_size.max(self.interleaved_buffer.len()),
            0.0,
        );

        (self.callback)(
            &mut self.interleaved_buffer[..interleaved_buffer_size],
            frame_size as u32,
            sample_rate,
            current_time,
        );

        // Returns a pointer to the raw interleaved buffer to Javascript so
        // it can deinterleave it into the output buffers.
        //
        // Deinterleaving is done on the Javascript side because it's simpler and it may be faster.
        // Doing it this way avoids an extra copy and the JS deinterleaving code
        // is likely heavily optimized by the browser's JS engine,
        // although I have not tested that assumption.
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
        *Box::from_raw(val as *mut _)
    }
}
