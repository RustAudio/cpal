extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

use self::js_sys::eval;
use self::wasm_bindgen::prelude::*;
use self::wasm_bindgen::JsCast;
use self::web_sys::{AudioContext, AudioContextOptions};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex, RwLock};
use traits::{DeviceTrait, HostTrait, StreamTrait};

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Host;

pub struct Stream {
    ctx: Arc<AudioContext>,
    on_ended_closures: Vec<Arc<RwLock<Option<Closure<dyn FnMut()>>>>>,
    config: StreamConfig,
    buffer_size_frames: usize,
}

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

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume this host is always available on webaudio.
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
        // TODO
        Ok(Vec::new().into_iter())
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
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    #[inline]
    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Device::supported_input_configs(self)
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Device::supported_output_configs(self)
    }

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_input_config(self)
    }

    #[inline]
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
        // TODO
        Err(BuildStreamError::StreamConfigNotSupported)
    }

    /// Create an output stream.
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

        let n_channels = config.channels as usize;

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
        let buffer_size_samples = buffer_size_frames * n_channels;
        let buffer_time_step_secs = buffer_time_step_secs(buffer_size_frames, config.sample_rate);

        let data_callback = Arc::new(Mutex::new(Box::new(data_callback)));

        // Create the WebAudio stream.
        let mut stream_opts = AudioContextOptions::new();
        stream_opts.sample_rate(config.sample_rate.0 as f32);
        let ctx = Arc::new(
            AudioContext::new_with_context_options(&stream_opts).map_err(
                |err| -> BuildStreamError {
                    let description = format!("{:?}", err);
                    let err = BackendSpecificError { description };
                    err.into()
                },
            )?,
        );

        // A container for managing the lifecycle of the audio callbacks.
        let mut on_ended_closures: Vec<Arc<RwLock<Option<Closure<dyn FnMut()>>>>> = Vec::new();

        // A cursor keeping track of the current time at which new frames should be scheduled.
        let time = Arc::new(RwLock::new(0f64));

        // Create a set of closures / callbacks which will continuously fetch and schedule sample
        // playback. Starting with two workers, e.g. a front and back buffer so that audio frames
        // can be fetched in the background.
        for _i in 0..2 {
            let data_callback_handle = data_callback.clone();
            let ctx_handle = ctx.clone();
            let time_handle = time.clone();

            // A set of temporary buffers to be used for intermediate sample transformation steps.
            let mut temporary_buffer = vec![0f32; buffer_size_samples];
            let mut temporary_channel_buffer = vec![0f32; buffer_size_frames];

            // Create a webaudio buffer which will be reused to avoid allocations.
            let ctx_buffer = ctx
                .create_buffer(
                    config.channels as u32,
                    buffer_size_frames as u32,
                    config.sample_rate.0 as f32,
                )
                .map_err(|err| -> BuildStreamError {
                    let description = format!("{:?}", err);
                    let err = BackendSpecificError { description };
                    err.into()
                })?;

            // A self reference to this closure for passing to future audio event calls.
            let on_ended_closure: Arc<RwLock<Option<Closure<dyn FnMut()>>>> =
                Arc::new(RwLock::new(None));
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
                        if *time_at_start_of_buffer > 0.001 {
                            *time_at_start_of_buffer
                        } else {
                            // 25ms of time to fetch the first sample data, increase to avoid
                            // initial underruns.
                            now + 0.025
                        }
                    };

                    // Populate the sample data into an interleaved temporary buffer.
                    {
                        let len = temporary_buffer.len();
                        let data = temporary_buffer.as_mut_ptr() as *mut ();
                        let mut data = unsafe { Data::from_parts(data, len, sample_format) };
                        let mut data_callback = data_callback_handle.lock().unwrap();
                        let callback = crate::StreamInstant::from_secs_f64(now);
                        let playback = crate::StreamInstant::from_secs_f64(time_at_start_of_buffer);
                        let timestamp = crate::OutputStreamTimestamp { callback, playback };
                        let info = OutputCallbackInfo { timestamp };
                        (data_callback.deref_mut())(&mut data, &info);
                    }

                    // Deinterleave the sample data and copy into the audio context buffer.
                    // We do not reference the audio context buffer directly e.g. getChannelData.
                    // As wasm-bindgen only gives us a copy, not a direct reference.
                    for channel in 0..n_channels {
                        for i in 0..buffer_size_frames {
                            temporary_channel_buffer[i] =
                                temporary_buffer[n_channels * i + channel];
                        }
                        ctx_buffer
                            .copy_to_channel(&mut temporary_channel_buffer, channel as i32)
                            .expect("Unable to write sample data into the audio context buffer");
                    }

                    // Create an AudioBufferSourceNode, schedule it to playback the reused buffer
                    // in the future.
                    let source = ctx_handle
                        .create_buffer_source()
                        .expect("Unable to create a webaudio buffer source");
                    source.set_buffer(Some(&ctx_buffer));
                    source
                        .connect_with_audio_node(&ctx_handle.destination())
                        .expect(
                        "Unable to connect the web audio buffer source to the context destination",
                    );
                    source.set_onended(Some(
                        on_ended_closure_handle
                            .read()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .as_ref()
                            .unchecked_ref(),
                    ));

                    source
                        .start_with_when(time_at_start_of_buffer)
                        .expect("Unable to start the webaudio buffer source");

                    // Keep track of when the next buffer worth of samples should be played.
                    *time_handle.write().unwrap() = time_at_start_of_buffer + buffer_time_step_secs;
                }) as Box<dyn FnMut()>));

            on_ended_closures.push(on_ended_closure);
        }

        Ok(Stream {
            ctx,
            on_ended_closures,
            config: config.clone(),
            buffer_size_frames,
        })
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let window = web_sys::window().unwrap();
        match self.ctx.resume() {
            Ok(_) => {
                // Begin webaudio playback, initially scheduling the closures to fire on a timeout
                // event.
                let mut offset_ms = 10;
                let time_step_secs =
                    buffer_time_step_secs(self.buffer_size_frames, self.config.sample_rate);
                let time_step_ms = (time_step_secs * 1_000.0) as i32;
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
            Err(err) => {
                let description = format!("{:?}", err);
                let err = BackendSpecificError { description };
                Err(err.into())
            }
        }
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        match self.ctx.suspend() {
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
    // TODO
    None
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
    if let Ok(audio_context_is_defined) = eval("typeof AudioContext !== 'undefined'") {
        audio_context_is_defined.as_bool().unwrap()
    } else {
        false
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

fn buffer_time_step_secs(buffer_size_frames: usize, sample_rate: SampleRate) -> f64 {
    buffer_size_frames as f64 / sample_rate.0 as f64
}
