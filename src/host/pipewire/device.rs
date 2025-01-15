use crate::host::pipewire::utils::{AudioBuffer, FromStreamConfigWithSampleFormat};
use crate::host::pipewire::Stream;
use crate::traits::DeviceTrait;
use crate::{BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, InputCallbackInfo, InputStreamTimestamp, OutputCallbackInfo, OutputStreamTimestamp, SampleFormat, SampleRate, StreamConfig, StreamError, StreamInstant, SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError};
use std::rc::Rc;
use std::time::Duration;
use pipewire_client::{AudioStreamInfo, Direction, PipewireClient, NodeInfo};
use pipewire_client::spa_utils::audio::raw::AudioInfoRaw;

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

#[derive(Debug, Clone)]
pub struct Device {
    pub(super) id: u32,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) nickname: String,
    pub(crate) direction: Direction,
    pub(super) is_default: bool,
    pub(crate) format: AudioInfoRaw,
    pub(super) client: Rc<PipewireClient>,
}

impl Device {
    pub(super) fn from(
        info: &NodeInfo,
        client: Rc<PipewireClient>,
    ) -> Result<Self, String> {
        Ok(Self {
            id: info.id.clone(),
            name: info.name.clone(),
            description: info.description.clone(),
            nickname: info.nickname.clone(),
            direction: info.direction.clone(),
            is_default: info.is_default.clone(),
            format: info.format.clone(),
            client,
        })
    }

    pub fn default_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let settings = match self.client.settings() {
            Ok(value) => value,
            Err(value) => return Err(DefaultStreamConfigError::BackendSpecific {
                err: BackendSpecificError {
                    description: value.description,
                }
            }),
        };
        Ok(SupportedStreamConfig {
            channels: *self.format.channels as u16,
            sample_rate: SampleRate(self.format.sample_rate.value),
            buffer_size: SupportedBufferSize::Range {
                min: settings.min_buffer_size,
                max: settings.max_buffer_size,
            },
            sample_format: self.format.sample_format.default.try_into()?,
        })
    }

    pub fn supported_configs(&self) -> Vec<SupportedStreamConfigRange> {
        let f = match self.default_config() {
            Err(_) => return vec![],
            Ok(f) => f,
        };
        let mut supported_configs = vec![];
        for &sample_format in self.format.sample_format.alternatives.iter() {
            supported_configs.push(SupportedStreamConfigRange {
                channels: f.channels,
                min_sample_rate: SampleRate(self.format.sample_rate.minimum),
                max_sample_rate: SampleRate(self.format.sample_rate.maximum),
                buffer_size: f.buffer_size.clone(),
                sample_format: sample_format.try_into().unwrap(),
            });
        }
        supported_configs
    }
    
    fn build_stream_raw<D, E> (
        &self,
        direction: Direction,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError> where
        D: FnMut(&mut Data) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let format: AudioStreamInfo = FromStreamConfigWithSampleFormat::from((config, sample_format));
        let channels = config.channels;
        let stream_name = self.client.create_stream(
            self.id,
            direction,
            format,
            move |buffer| {
                let mut buffer = AudioBuffer::from(
                    buffer, 
                    sample_format,
                    channels
                );
                let data = buffer.data();
                if data.is_none() {
                    return;
                }
                let mut data = data.unwrap();
                data_callback(&mut data)
            }
        ).unwrap();
        Ok(Stream::new(stream_name, self.client.clone()))
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.nickname.clone())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Ok(self.supported_configs().into_iter())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Ok(self.supported_configs().into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config()
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config()
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.build_stream_raw(
            Direction::Input,
            config,
            sample_format,
            move |data| {
                data_callback(
                    data,
                    &InputCallbackInfo {
                        timestamp: InputStreamTimestamp {
                            callback: StreamInstant::from_nanos(0),
                            capture: StreamInstant::from_nanos(0),
                        },
                    }
                )
            },
            error_callback,
            timeout
        )
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        self.build_stream_raw(
            Direction::Output,
            config,
            sample_format,
            move |data| {
                data_callback(
                    data,
                    &OutputCallbackInfo {
                        timestamp: OutputStreamTimestamp {
                            callback: StreamInstant::from_nanos(0),
                            playback: StreamInstant::from_nanos(0),
                        },
                    }
                )
            },
            error_callback,
            timeout
        )
    }
}
