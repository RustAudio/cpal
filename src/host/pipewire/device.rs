use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use crate::{
    traits::DeviceTrait, BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError,
    InputCallbackInfo, OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

use super::{Message, Stream, SupportedInputConfigs, SupportedOutputConfigs};

static LAST_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub struct Device {
    pub(super) tx: pipewire::channel::Sender<Message>,
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("null".to_owned())
    }

    #[inline]
    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        Ok(vec![SupportedStreamConfigRange {
            channels: 2,
            min_sample_rate: SampleRate(44100),
            max_sample_rate: SampleRate(44100),
            buffer_size: SupportedBufferSize::Range {
                min: 15053,
                max: 15053,
            },
            sample_format: SampleFormat::I16,
        }]
        .into_iter())
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(vec![SupportedStreamConfigRange {
            channels: 2,
            min_sample_rate: SampleRate(44100),
            max_sample_rate: SampleRate(44100),
            buffer_size: SupportedBufferSize::Range {
                min: 15053,
                max: 15053,
            },
            sample_format: SampleFormat::I16,
        }]
        .into_iter())
    }

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Ok(SupportedStreamConfig {
            channels: 2,
            sample_rate: SampleRate(44100),
            buffer_size: SupportedBufferSize::Range {
                min: 15053,
                max: 15053,
            },
            sample_format: SampleFormat::I16,
        })
    }

    #[inline]
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Ok(SupportedStreamConfig {
            channels: 2,
            sample_rate: SampleRate(44100),
            buffer_size: SupportedBufferSize::Range {
                min: 15053,
                max: 15053,
            },
            sample_format: SampleFormat::I16,
        })
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        _error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let id = LAST_ID.fetch_add(1, Ordering::Relaxed);
        self.tx.send(Message::CreateInputStream {
            id,
            config: config.clone(),
            sample_format,
            data_callback: Box::new(data_callback),
        });
        Ok(Stream {
            id,
            tx: self.tx.clone(),
        })
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
        let id = LAST_ID.fetch_add(1, Ordering::Relaxed);
        self.tx.send(Message::CreateOutputStream {
            id,
            config: config.clone(),
            sample_format,
            data_callback: Box::new(data_callback),
        });
        Ok(Stream {
            id,
            tx: self.tx.clone(),
        })
    }
}
