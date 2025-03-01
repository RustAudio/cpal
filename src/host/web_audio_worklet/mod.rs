mod dependent_module;
use js_sys::wasm_bindgen;

use crate::dependent_module;
use wasm_bindgen::prelude::*;

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError,
    DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError,
    SampleFormat, SampleRate, StreamConfig, StreamError, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use std::time::Duration;

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Host;

pub struct Stream {
    audio_context: web_sys::AudioContext,
}

pub type SupportedInputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;

const MIN_CHANNELS: u16 = 1;
const MAX_CHANNELS: u16 = 32;
const MIN_SAMPLE_RATE: SampleRate = SampleRate(8_000);
const MAX_SAMPLE_RATE: SampleRate = SampleRate(96_000);
const DEFAULT_SAMPLE_RATE: SampleRate = SampleRate(44_100);
const SUPPORTED_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        if let Ok(audio_context_is_defined) = js_sys::eval("typeof AudioWorklet !== 'undefined'") {
            audio_context_is_defined.as_bool().unwrap()
        } else {
            false
        }
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
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
    fn new() -> Result<Self, DevicesError> {
        Ok(Self::default())
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("Default Device".to_owned())
    }

    #[inline]
    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        // TODO
        Ok(Vec::new().into_iter())
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        let buffer_size = SupportedBufferSize::Unknown;

        // In actuality the number of supported channels cannot be fully known until
        // the browser attempts to initialized the AudioWorklet.

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

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        // TODO
        Err(DefaultStreamConfigError::StreamTypeNotSupported)
    }

    #[inline]
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

    fn build_input_stream_raw<D, E>(
        &self,
        _config: &StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // TODO
        Err(BuildStreamError::StreamConfigNotSupported)
    }

    /// Create an output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        mut error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        if !valid_config(config, sample_format) {
            return Err(BuildStreamError::StreamConfigNotSupported);
        }

        let config = config.clone();

        let stream_opts = web_sys::AudioContextOptions::new();
        stream_opts.set_sample_rate(config.sample_rate.0 as f32);

        let audio_context = web_sys::AudioContext::new_with_context_options(&stream_opts).map_err(
            |err| -> BuildStreamError {
                let description = format!("{:?}", err);
                let err = BackendSpecificError { description };
                err.into()
            },
        )?;

        let destination = audio_context.destination();

        // If possible, set the destination's channel_count to the given config.channel.
        // If not, fallback on the default destination channel_count to keep previous behavior
        // and do not return an error.
        if config.channels as u32 <= destination.max_channel_count() {
            destination.set_channel_count(config.channels as u32);
        }

        let ctx = audio_context.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result: Result<(), JsValue> = (async move || {
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
                            let data = interleaved_data.as_mut_ptr() as *mut ();
                            let mut data = unsafe {
                                Data::from_parts(data, interleaved_data.len(), sample_format)
                            };

                            let callback = crate::StreamInstant::from_secs_f64(now);

                            let buffer_duration =
                                frames_to_duration(frame_size as _, SampleRate(sample_rate as u32));
                            let playback = callback.add(buffer_duration).expect(
                            "`playback` occurs beyond representation supported by `StreamInstant`",
                        );
                            let timestamp = crate::OutputStreamTimestamp { callback, playback };
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
            })()
            .await;

            if let Err(e) = result {
                let description = if let Some(string_value) = e.as_string() {
                    string_value
                } else {
                    format!("Browser error initializing stream: {:?}", e)
                };

                error_callback(StreamError::BackendSpecific {
                    err: BackendSpecificError { description },
                })
            }
        });

        Ok(Stream { audio_context })
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        match self.audio_context.resume() {
            Ok(_) => Ok(()),
            Err(err) => {
                let description = format!("{:?}", err);
                let err = BackendSpecificError { description };
                Err(err.into())
            }
        }
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        match self.audio_context.suspend() {
            Ok(_) => Ok(()),
            Err(err) => {
                let description = format!("{:?}", err);
                let err = BackendSpecificError { description };
                Err(err.into())
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.audio_context.close();
    }
}

impl Default for Devices {
    fn default() -> Devices {
        Devices(true)
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

// Whether or not the given stream configuration is valid for building a stream.
fn valid_config(conf: &StreamConfig, sample_format: SampleFormat) -> bool {
    conf.channels <= MAX_CHANNELS
        && conf.channels >= MIN_CHANNELS
        && conf.sample_rate <= MAX_SAMPLE_RATE
        && conf.sample_rate >= MIN_SAMPLE_RATE
        && sample_format == SUPPORTED_SAMPLE_FORMAT
}

// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

/// WasmAudioProcessor provides an interface for the Javascript code
/// running in the AudioWorklet to interact with Rust.
#[wasm_bindgen]
pub struct WasmAudioProcessor {
    #[wasm_bindgen(skip)]
    interleaved_buffer: Vec<f32>,
    #[wasm_bindgen(skip)]
    // Passes in an interleaved scratch buffer, frame size, sample rate, and current time.
    callback: Box<dyn FnMut(&mut [f32], u32, u32, f64)>,
}

impl WasmAudioProcessor {
    pub fn new(callback: Box<dyn FnMut(&mut [f32], u32, u32, f64)>) -> Self {
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

    pub fn pack(self) -> usize {
        Box::into_raw(Box::new(self)) as usize
    }
    pub unsafe fn unpack(val: usize) -> Self {
        *Box::from_raw(val as *mut _)
    }
}
