extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

mod bridge;

use js_sys::Array;
use js_sys::Float32Array;

use self::js_sys::eval;
use self::wasm_bindgen::prelude::*;
use self::wasm_bindgen::JsCast;
use self::web_sys::{AudioContext, AudioContextOptions};
use crate::host::webaudio::bridge::WebAudioBridge;
use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Host;

pub struct Stream {
    ctx: Arc<AudioContext>,
    bridge: Arc<WebAudioBridge>,
    config: StreamConfig,
}

pub type SupportedInputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;

const MIN_CHANNELS: u16 = 1;
const MAX_CHANNELS: u16 = 32;
const MIN_SAMPLE_RATE: SampleRate = SampleRate(8_000);
const MAX_SAMPLE_RATE: SampleRate = SampleRate(96_000);
const DEFAULT_SAMPLE_RATE: SampleRate = SampleRate(44_100);
// audio processor node is called with exactly 128 frames
// https://developer.mozilla.org/en-US/docs/Web/API/AudioWorkletProcessor#deriving_classes
const DEFAULT_BUFFER_SIZE: u32 = 128;
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
        if is_media_devices_available() {
            let buffer_size = SupportedBufferSize::Range {
                min: DEFAULT_BUFFER_SIZE,
                max: DEFAULT_BUFFER_SIZE,
            };
            let configs: Vec<_> = vec![SupportedStreamConfigRange {
                channels: 1,
                min_sample_rate: MIN_SAMPLE_RATE,
                max_sample_rate: MAX_SAMPLE_RATE,
                buffer_size: buffer_size.clone(),
                sample_format: SUPPORTED_SAMPLE_FORMAT,
            }];
            Ok(configs.into_iter())
        } else {
            Err(SupportedStreamConfigsError::BackendSpecific {
                err: BackendSpecificError {
                    description: "navigator.mediaDevices is only available in secure contexts"
                        .to_string(),
                },
            })
        }
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        let buffer_size = SupportedBufferSize::Range {
            min: DEFAULT_BUFFER_SIZE,
            max: DEFAULT_BUFFER_SIZE,
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
        let err = DefaultStreamConfigError::StreamTypeNotSupported;
        match self.supported_input_configs() {
            Ok(mut c) => c
                .nth(0)
                .map(|c| c.with_sample_rate(DEFAULT_SAMPLE_RATE))
                .ok_or(err),
            Err(e) => Err(match e {
                SupportedStreamConfigsError::DeviceNotAvailable => {
                    DefaultStreamConfigError::DeviceNotAvailable
                }
                SupportedStreamConfigsError::InvalidArgument => {
                    DefaultStreamConfigError::StreamTypeNotSupported
                }
                SupportedStreamConfigsError::BackendSpecific { err } => err.into(),
            }),
        }
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

        // Create the WebAudio stream.

        let mut stream_opts = AudioContextOptions::new();
        stream_opts.sample_rate(config.sample_rate.0 as f32);
        let ctx = AudioContext::new_with_context_options(&stream_opts)
            .map_err(map_js_err::<BuildStreamError>)?;

        let ctx = Arc::new(ctx);

        let mut bridge = WebAudioBridge::new(ctx.clone(), config.channels, DEFAULT_BUFFER_SIZE)?;

        let data_callback = Arc::new(Mutex::new(Box::new(data_callback)));

        let destination = ctx.destination();

        // If possible, set the destination's channel_count to the given config.channel.
        // If not, fallback on the default destination channel_count to keep previous behavior
        // and do not return an error.
        if config.channels as u32 <= destination.max_channel_count() {
            destination.set_channel_count(config.channels as u32);
        }

        _ = bridge.connect_with_audio_node(Arc::new(destination.into()));

        // A cursor keeping track of the current time at which new frames should be scheduled.
        let time = Arc::new(RwLock::new(0f64));
        let n_channels = config.channels as usize;

        let buffer_size_frames = match config.buffer_size {
            BufferSize::Fixed(v) => {
                if v == 0 {
                    return Err(BuildStreamError::StreamConfigNotSupported);
                } else {
                    v as usize
                }
            }
            BufferSize::Default => DEFAULT_BUFFER_SIZE as usize,
        };
        let buffer_size_samples = buffer_size_frames * n_channels;
        let buffer_time_step_secs = buffer_time_step_secs(buffer_size_frames, config.sample_rate);
        let mut temporary_buffer = vec![0f32; buffer_size_samples];

        let ctx_handle = ctx.clone();
        let time_handle = time.clone();
        let producer = Box::new(move |floats: &Float32Array| {
            log::debug!("produce output cb tick");
            let now = ctx_handle.current_time();
            let time_at_start_of_buffer = {
                let time_at_start_of_buffer = time_handle
                    .read()
                    .expect("Unable to get a read lock on the time cursor");
                // Synchronize first buffer as necessary (eg. keep the time value
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
            let len = temporary_buffer.len();
            let data = temporary_buffer.as_mut_ptr() as *mut ();
            let mut data = unsafe { Data::from_parts(data, len, sample_format) };
            let mut data_callback = data_callback.lock().unwrap();
            let callback = crate::StreamInstant::from_secs_f64(now);
            let playback = crate::StreamInstant::from_secs_f64(time_at_start_of_buffer);
            let timestamp = crate::OutputStreamTimestamp { callback, playback };
            let info = OutputCallbackInfo { timestamp };

            // call the data callback
            (data_callback.deref_mut())(&mut data, &info);

            // tick the clock
            *time_handle.write().unwrap() = time_at_start_of_buffer + buffer_time_step_secs;

            // update bridge buffer
            floats.copy_from(temporary_buffer.as_slice());
        }) as Box<dyn FnMut(&Float32Array)>;

        bridge.register_output_callback(producer)?;

        let js_bridge = Arc::new(bridge);

        Ok(Stream {
            ctx,
            bridge: js_bridge,
            config: config.clone(),
        })
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let window = web_sys::window().unwrap();
        match self.ctx.resume() {
            Ok(_) => self.bridge.schedule_next_tick(),
            Err(err) => {
                let description = format!("{:?}", err);
                let err = BackendSpecificError { description };
                Err(err.into())
            }
        }
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        match self.ctx.suspend() {
            Ok(_) => self.bridge.cancel_next_tick(),
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
        _ = self.bridge.cancel_next_tick();
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
            Some(Device::default())
        } else {
            None
        }
    }
}

#[inline]
fn default_input_device() -> Option<Device> {
    if is_media_devices_available() {
        Some(Device::default())
    } else {
        None
    }
}

#[inline]
fn default_output_device() -> Option<Device> {
    if is_webaudio_available() {
        Some(Device::default())
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

// Detects whether the `navigator.mediaDevices` global variable is available.
// https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia#privacy_and_security
// only available in secure contexts
fn is_media_devices_available() -> bool {
    if let Ok(media_devices_is_defined) = eval("typeof navigator.mediaDevices !== 'undefined'") {
        media_devices_is_defined.as_bool().unwrap()
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

fn map_js_err<E>(err: JsValue) -> E
where
    E: From<BackendSpecificError>,
{
    let description = format!("{:?}", err);
    let err = BackendSpecificError { description };
    E::from(err)
}
