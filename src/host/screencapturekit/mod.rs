use std::{cell::RefCell, rc::Rc, time::Duration};

use crate::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DevicesError,
    InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat,
    SampleRate, StreamConfig, StreamError, StreamInstant, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};

use cidre::{
    arc::Retained,
    cm, define_obj_type, dispatch, ns, objc,
    sc::{self, StreamOutput, StreamOutputImpl},
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
    display: Retained<sc::Display>,
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
    pub fn new(display: Retained<sc::Display>) -> Self {
        Self { display }
    }

    fn name(&self) -> String {
        format!("Display {}", self.display.display_id())
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
        let queue = dispatch::Queue::serial_with_ar_pool();
        let mut cfg = sc::StreamCfg::new();
        cfg.set_captures_audio(true);
        cfg.set_excludes_current_process_audio(false);
        let windows = ns::Array::new();
        let filter = sc::ContentFilter::with_display_excluding_windows(&self.display, &windows);
        let sc_stream = sc::Stream::new(&filter, &cfg);
        let inner = CapturerInner {
            current_data: vec![],
            config: config.clone(),
            sample_format,
            data_callback: Box::new(data_callback),
            error_callback: Box::new(error_callback),
        };
        let capturer = Capturer::with(inner);
        sc_stream
            .add_stream_output(capturer.as_ref(), sc::OutputType::Audio, Some(&queue))
            .map_err(|e| BackendSpecificError {
                description: format!("{e}"),
            })?;

        Ok(Stream::new(StreamInner {
            _capturer: capturer,
            sc_stream,
            playing: false,
        }))
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
    // Keep capturer alive
    _capturer: Retained<Capturer>,
    sc_stream: Retained<sc::Stream>,
    playing: bool,
}

#[derive(Clone)]
pub struct Stream {
    inner: Rc<RefCell<StreamInner>>,
}

impl Stream {
    fn new(inner: StreamInner) -> Self {
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let mut stream = self.inner.borrow_mut();
        if !stream.playing {
            let (tx, rx) = std::sync::mpsc::channel();
            stream.sc_stream.start_with_ch(move |e| {
                let res = if let Some(e) = e {
                    Result::Err(BackendSpecificError {
                        description: format!("{e}"),
                    })
                } else {
                    Result::Ok(())
                };
                tx.send(res).unwrap();
            });
            rx.recv().unwrap()?;
            stream.playing = true;
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        let mut stream = self.inner.borrow_mut();
        if stream.playing {
            let (tx, rx) = std::sync::mpsc::channel();
            stream.sc_stream.stop_with_ch(move |e| {
                let res = if let Some(e) = e {
                    Result::Err(BackendSpecificError {
                        description: format!("{e}"),
                    })
                } else {
                    Result::Ok(())
                };
                tx.send(res).unwrap();
            });
            rx.recv().unwrap()?;
            stream.playing = false;
        }
        Ok(())
    }
}

#[repr(C)]
struct CapturerInner {
    current_data: Vec<f32>,
    config: StreamConfig,
    sample_format: SampleFormat,
    data_callback: Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>,
    error_callback: Box<dyn FnMut(StreamError) + Send + 'static>,
}

impl CapturerInner {
    fn handle_audio(&mut self, sample_buf: &mut cm::SampleBuf) {
        let start = std::time::Instant::now();
        // Assume 2 channels
        let buf_list = match sample_buf.audio_buf_list::<2>() {
            Ok(res) => res,
            Err(e) => {
                (self.error_callback)(StreamError::BackendSpecific {
                    err: BackendSpecificError {
                        description: format!("{e}"),
                    },
                });
                return;
            }
        };
        let buf_list = buf_list.list();
        let buf_cnt = buf_list.number_buffers as usize;
        let buf_len =
            buf_list.buffers[0].data_bytes_size as usize / self.sample_format.sample_size();
        let required_len = buf_cnt * buf_len;

        if required_len > self.current_data.len() {
            self.current_data.resize(required_len, 0.0);
        }

        for (i, buf) in buf_list.buffers.iter().enumerate() {
            // Assume f32 sample format
            let buf_data = unsafe { std::slice::from_raw_parts(buf.data as *const f32, buf_len) };
            for (item, v) in self
                .current_data
                .iter_mut()
                .skip(i)
                .step_by(2)
                .zip(buf_data.iter())
            {
                *item = *v;
            }
        }

        let data = self.current_data.as_mut_ptr() as *mut ();
        let data = unsafe { Data::from_parts(data, required_len, self.sample_format) };

        let capture = host_time_to_stream_instant(sample_buf.pts());
        let duration = frames_to_duration(buf_len, self.config.sample_rate);
        let elapsed = start.elapsed();
        let callback = capture.add(duration).unwrap().add(elapsed).unwrap();
        let timestamp = crate::InputStreamTimestamp { callback, capture };
        let info = InputCallbackInfo { timestamp };
        (self.data_callback)(&data, &info);
    }
}

define_obj_type!(Capturer + StreamOutputImpl, CapturerInner, CAPTURER);

impl StreamOutput for Capturer {}

#[objc::add_methods]
impl StreamOutputImpl for Capturer {
    extern "C" fn impl_stream_did_output_sample_buf(
        &mut self,
        _cmd: Option<&cidre::objc::Sel>,
        _stream: &sc::Stream,
        sample_buf: &mut cm::SampleBuf,
        kind: sc::OutputType,
    ) {
        match kind {
            sc::OutputType::Audio => self.inner_mut().handle_audio(sample_buf),
            _ => {}
        }
    }
}

fn host_time_to_stream_instant(cm_time: cm::Time) -> StreamInstant {
    let secs = cm_time.value / cm_time.scale as i64;
    let subsec_nanos =
        (cm_time.value % cm_time.scale as i64) * 1_000_000_000 / cm_time.scale as i64;
    StreamInstant::new(secs, subsec_nanos as u32)
}

fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}
