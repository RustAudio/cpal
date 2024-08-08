use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use screencapturekit::{
    cm_sample_buffer::CMSampleBuffer,
    sc_content_filter::{InitParams, SCContentFilter},
    sc_display::SCDisplay,
    sc_error_handler::StreamErrorHandler,
    sc_output_handler::{SCStreamOutputType, StreamOutput},
    sc_stream::SCStream,
    sc_stream_configuration::SCStreamConfiguration,
    sc_types::base::{CMTime, CMTimeScale},
};
use screencapturekit_sys::audio_buffer::CopiedAudioBuffer;

use crate::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DevicesError,
    InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat,
    SampleRate, StreamConfig, StreamError, StreamInstant, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};

pub use enumerate::{
    default_input_device, default_output_device, Devices, SupportedInputConfigs,
    SupportedOutputConfigs,
};

pub mod enumerate;

#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;

    type Device = Device;

    fn is_available() -> bool {
        // Assume screencapturekit is always available
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

#[derive(Clone)]
pub struct Device {
    display: SCDisplay,
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;

    type SupportedOutputConfigs = SupportedOutputConfigs;

    type Stream = Stream;

    fn name(&self) -> Result<String, crate::DeviceNameError> {
        Ok(self.name().clone())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Self::supported_input_configs(self)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Self::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Self::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Self::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Self::build_input_stream(self, config, sample_format, data_callback, error_callback)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Self::build_output_stream(
            self,
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }
}

impl Device {
    pub fn new(display: SCDisplay) -> Self {
        Self { display }
    }

    fn name(&self) -> String {
        format!("Display {}", self.display.display_id)
    }

    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        let channels = 2;
        let min_sample_rate = SampleRate(48000);
        let max_sample_rate = SampleRate(48000);
        let buffer_size = SupportedBufferSize::Unknown;
        let sample_format = SampleFormat::F32;
        let supported_configs = vec![SupportedStreamConfigRange {
            channels,
            min_sample_rate,
            max_sample_rate,
            buffer_size,
            sample_format,
        }];
        Ok(supported_configs.into_iter())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(Vec::new().into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let config = Self::supported_input_configs(self)
            .expect("failed to get supported input configs")
            .next()
            .expect("no supported input configs")
            .with_max_sample_rate();
        Ok(config)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Err(DefaultStreamConfigError::StreamTypeNotSupported)
    }

    fn build_input_stream<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let width = 2;
        let height = 2;
        let minimum_frame_interval = CMTime {
            value: 1,
            timescale: CMTimeScale::MAX,
            flags: 0,
            epoch: 0,
        };
        let captures_audio = true;
        let sample_rate = config.sample_rate.0;
        let channel_count = config.channels as u32;

        let sc_stream_config = SCStreamConfiguration {
            width,
            height,
            minimum_frame_interval,
            captures_audio,
            sample_rate,
            channel_count,
            ..Default::default()
        };
        let init_params = InitParams::Display(self.display.clone());
        let filter = SCContentFilter::new(init_params);
        let error_callback = Arc::new(Mutex::new(error_callback));
        let mut sc_stream = SCStream::new(
            filter,
            sc_stream_config,
            ErrorHandler(error_callback.clone()),
        );
        let output = Capturer {
            stream_config: config.clone(),
            sample_format,
            data_callback: Arc::new(Mutex::new(data_callback)),
        };
        sc_stream.add_output(output, SCStreamOutputType::Audio);
        let playing = false;
        let stream = Stream::new(StreamInner { sc_stream, playing });
        Ok(stream)
    }

    fn build_output_stream<D, E>(
        &self,
        _config: &StreamConfig,
        _sample_format: SampleFormat,
        _data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Err(BuildStreamError::StreamConfigNotSupported)
    }
}

struct StreamInner {
    sc_stream: SCStream,
    playing: bool,
}

#[derive(Clone)]
pub struct Stream {
    inner: Arc<Mutex<StreamInner>>,
}

impl Stream {
    fn new(inner: StreamInner) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let mut stream = self.inner.lock().unwrap();
        if !stream.playing {
            stream
                .sc_stream
                .start_capture()
                .map_err(|description| BackendSpecificError { description })?;
            stream.playing = true;
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        let mut stream = self.inner.lock().unwrap();
        if stream.playing {
            stream
                .sc_stream
                .stop_capture()
                .map_err(|description| BackendSpecificError { description })?;
            stream.playing = false;
        }
        Ok(())
    }
}

struct Capturer {
    stream_config: StreamConfig,
    sample_format: SampleFormat,
    data_callback: Arc<Mutex<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>>,
}

impl StreamOutput for Capturer {
    fn did_output_sample_buffer(&self, sample_buffer: CMSampleBuffer, of_type: SCStreamOutputType) {
        match of_type {
            SCStreamOutputType::Audio => self.did_output_audio_sample_buffer(sample_buffer),
            SCStreamOutputType::Screen => {}
        }
    }
}

impl Capturer {
    fn did_output_audio_sample_buffer(&self, sample_buffer: CMSampleBuffer) {
        let buffers = sample_buffer.sys_ref.get_av_audio_buffer_list();
        let channels = buffers[0].number_channels;
        let mut data = raw_buffers_to_stream_data(buffers);
        let len = data.len();
        let data = data.as_mut_ptr() as *mut ();
        let data = unsafe { Data::from_parts(data, len, self.sample_format) };

        let cm_time = sample_buffer.sys_ref.get_presentation_timestamp();
        let buffer_frames = len / channels as usize;
        let callback = host_time_to_stream_instant(cm_time);
        let delay = frames_to_duration(buffer_frames, self.stream_config.sample_rate);
        let capture = callback
            .sub(delay)
            .expect("`capture` occurs before origin of alsa `StreamInstant`");
        let timestamp = crate::InputStreamTimestamp { callback, capture };
        let info = InputCallbackInfo { timestamp };

        let mut data_callback = self.data_callback.lock().unwrap();
        data_callback(&data, &info);
    }
}

struct ErrorHandler(Arc<Mutex<dyn FnMut(StreamError) + Send + 'static>>);
impl StreamErrorHandler for ErrorHandler {
    fn on_error(&self) {
        let mut error_callback = self.0.lock().unwrap();
        error_callback(StreamError::BackendSpecific {
            err: BackendSpecificError {
                description: "error occurred in screencapturekit stream".to_string(),
            },
        });
    }
}

fn host_time_to_stream_instant(cm_time: CMTime) -> StreamInstant {
    let secs = cm_time.value / cm_time.timescale as i64;
    let subsec_nanos =
        (cm_time.value % cm_time.timescale as i64) * 1_000_000_000 / cm_time.timescale as i64;
    StreamInstant::new(secs, subsec_nanos as u32)
}

// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

fn raw_buffers_to_stream_data(buffers: Vec<CopiedAudioBuffer>) -> Vec<f32> {
    let buffer_num = buffers.len();
    let buffer_data_len = buffers[0].data.len() / std::mem::size_of::<f32>();
    let mut stream_data = vec![0.0; buffer_num * buffer_data_len];
    for (i, buffer) in buffers.into_iter().enumerate() {
        let buffer_data = buffer.data;
        let buffer_data = buffer_data.as_ptr() as *const f32;
        let buffer_data = unsafe { std::slice::from_raw_parts(buffer_data, buffer_data_len) };
        for (j, &sample) in buffer_data.iter().enumerate() {
            stream_data[j * buffer_num + i] = sample;
        }
    }
    stream_data
}
