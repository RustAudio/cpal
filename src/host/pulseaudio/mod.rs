use futures::executor::block_on;
use pulseaudio::protocol;

mod stream;

pub use stream::Stream;

use crate::{
    traits::{DeviceTrait, HostTrait},
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DeviceDescription,
    DeviceDescriptionBuilder, DeviceDirection, DeviceId, DeviceIdError, DeviceNameError,
    DevicesError, HostId, HostUnavailable, InputCallbackInfo, OutputCallbackInfo, SampleFormat,
    SampleRate, StreamConfig, StreamError, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange, SupportedStreamConfigsError,
};

const PULSE_FORMATS: &[SampleFormat] = &[
    SampleFormat::U8,
    SampleFormat::I16,
    SampleFormat::I24,
    SampleFormat::I32,
    SampleFormat::F32,
];

impl TryFrom<protocol::SampleFormat> for SampleFormat {
    type Error = ();

    fn try_from(spec: protocol::SampleFormat) -> Result<Self, Self::Error> {
        match spec {
            protocol::SampleFormat::U8 => Ok(SampleFormat::U8),
            protocol::SampleFormat::S16Le | protocol::SampleFormat::S16Be => Ok(SampleFormat::I16),
            protocol::SampleFormat::S24Le | protocol::SampleFormat::S24Be => Ok(SampleFormat::I24),
            protocol::SampleFormat::S32Le | protocol::SampleFormat::S32Be => Ok(SampleFormat::I32),
            protocol::SampleFormat::Float32Le | protocol::SampleFormat::Float32Be => {
                Ok(SampleFormat::F32)
            }
            _ => Err(()),
        }
    }
}

impl TryFrom<SampleFormat> for protocol::SampleFormat {
    type Error = ();

    fn try_from(format: SampleFormat) -> Result<Self, Self::Error> {
        match (format, cfg!(target_endian = "little")) {
            (SampleFormat::U8, _) => Ok(protocol::SampleFormat::U8),
            (SampleFormat::I16, true) => Ok(protocol::SampleFormat::S16Le),
            (SampleFormat::I16, false) => Ok(protocol::SampleFormat::S16Be),
            (SampleFormat::I24, true) => Ok(protocol::SampleFormat::S24Le),
            (SampleFormat::I24, false) => Ok(protocol::SampleFormat::S24Be),
            (SampleFormat::I32, true) => Ok(protocol::SampleFormat::S32Le),
            (SampleFormat::I32, false) => Ok(protocol::SampleFormat::S32Be),
            (SampleFormat::F32, true) => Ok(protocol::SampleFormat::Float32Le),
            (SampleFormat::F32, false) => Ok(protocol::SampleFormat::Float32Be),
            _ => Err(()),
        }
    }
}

impl From<pulseaudio::ClientError> for BackendSpecificError {
    fn from(err: pulseaudio::ClientError) -> Self {
        BackendSpecificError {
            description: err.to_string(),
        }
    }
}

/// A Host for connecting to the popular PulseAudio and PipeWire (via
/// pipewire-pulse) audio servers on linux.
pub struct Host {
    client: pulseaudio::Client,
}

impl Host {
    pub fn new() -> Result<Self, HostUnavailable> {
        let client =
            pulseaudio::Client::from_env(c"cpal-pulseaudio").map_err(|_| HostUnavailable)?;

        Ok(Self { client })
    }
}

impl HostTrait for Host {
    type Devices = std::vec::IntoIter<Device>;
    type Device = Device;

    fn is_available() -> bool {
        pulseaudio::socket_path_from_env().is_some()
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        let sinks = block_on(self.client.list_sinks()).map_err(|err| BackendSpecificError {
            description: format!("Failed to list sinks: {err}"),
        })?;

        let sources = block_on(self.client.list_sources()).map_err(|err| BackendSpecificError {
            description: format!("Failed to list sources: {err}"),
        })?;

        Ok(sinks
            .into_iter()
            .map(|sink_info| Device::Sink {
                client: self.client.clone(),
                info: sink_info,
            })
            .chain(sources.into_iter().map(|source_info| Device::Source {
                client: self.client.clone(),
                info: source_info,
            }))
            .collect::<Vec<_>>()
            .into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        let source_info = block_on(
            self.client
                .source_info_by_name(protocol::DEFAULT_SOURCE.to_owned()),
        )
        .ok()?;

        Some(Device::Source {
            client: self.client.clone(),
            info: source_info,
        })
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        let sink_info = block_on(
            self.client
                .sink_info_by_name(protocol::DEFAULT_SINK.to_owned()),
        )
        .ok()?;

        Some(Device::Sink {
            client: self.client.clone(),
            info: sink_info,
        })
    }
}

/// A PulseAudio sink or source.
#[derive(Debug, Clone)]
pub enum Device {
    Sink {
        client: pulseaudio::Client,
        info: protocol::SinkInfo,
    },
    Source {
        client: pulseaudio::Client,
        info: protocol::SourceInfo,
    },
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
    type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        let name = match self {
            Device::Sink { info, .. } => &info.name,
            Device::Source { info, .. } => &info.name,
        };

        Ok(String::from_utf8_lossy(name.as_bytes()).into_owned())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        let Device::Source { .. } = self else {
            return Ok(vec![].into_iter());
        };

        let mut ranges = vec![];
        for format in PULSE_FORMATS {
            for channel_count in 1..protocol::sample_spec::MAX_CHANNELS {
                ranges.push(SupportedStreamConfigRange {
                    channels: channel_count as _,
                    min_sample_rate: SampleRate(1),
                    max_sample_rate: SampleRate(protocol::sample_spec::MAX_RATE),
                    buffer_size: SupportedBufferSize::Range {
                        min: 0,
                        max: protocol::MAX_MEMBLOCKQ_LENGTH as _,
                    },
                    sample_format: *format,
                })
            }
        }

        Ok(ranges.into_iter())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        let Device::Sink { .. } = self else {
            return Ok(vec![].into_iter());
        };

        let mut ranges = vec![];
        for format in PULSE_FORMATS {
            for channel_count in 1..protocol::sample_spec::MAX_CHANNELS {
                ranges.push(SupportedStreamConfigRange {
                    channels: channel_count as _,
                    min_sample_rate: SampleRate(1),
                    max_sample_rate: SampleRate(protocol::sample_spec::MAX_RATE),
                    buffer_size: SupportedBufferSize::Range {
                        min: 0,
                        max: protocol::MAX_MEMBLOCKQ_LENGTH as _,
                    },
                    sample_format: *format,
                })
            }
        }

        Ok(ranges.into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let Device::Source { info, .. } = self else {
            return Err(DefaultStreamConfigError::StreamTypeNotSupported);
        };

        Ok(SupportedStreamConfig {
            channels: info.channel_map.num_channels() as _,
            sample_rate: SampleRate(info.sample_spec.sample_rate),
            buffer_size: SupportedBufferSize::Range {
                min: 0,
                max: protocol::MAX_MEMBLOCKQ_LENGTH as _,
            },
            sample_format: info
                .sample_spec
                .format
                .try_into()
                .unwrap_or(SampleFormat::F32),
        })
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let Device::Sink { info, .. } = self else {
            return Err(DefaultStreamConfigError::StreamTypeNotSupported);
        };

        Ok(SupportedStreamConfig {
            channels: info.channel_map.num_channels() as _,
            sample_rate: SampleRate(info.sample_spec.sample_rate),
            buffer_size: SupportedBufferSize::Range {
                min: 0,
                max: protocol::MAX_MEMBLOCKQ_LENGTH as _,
            },
            sample_format: info
                .sample_spec
                .format
                .try_into()
                .unwrap_or(SampleFormat::F32),
        })
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
        let Device::Source { client, info } = self else {
            return Err(BuildStreamError::StreamConfigNotSupported);
        };

        let format: protocol::SampleFormat = sample_format
            .try_into()
            .map_err(|_| BuildStreamError::StreamConfigNotSupported)?;

        let sample_spec = make_sample_spec(config, format);
        let channel_map = make_channel_map(config);
        let buffer_attr = make_buffer_attr(config, format);

        let params = protocol::RecordStreamParams {
            sample_spec,
            channel_map,
            source_index: Some(info.index),
            buffer_attr,
            flags: protocol::stream::StreamFlags {
                // Start the stream suspended.
                start_corked: true,
                ..Default::default()
            },
            ..Default::default()
        };

        stream::Stream::new_record(client.clone(), params, data_callback, error_callback)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let Device::Sink { client, info } = self else {
            return Err(BuildStreamError::StreamConfigNotSupported);
        };

        let format: protocol::SampleFormat = sample_format
            .try_into()
            .map_err(|_| BuildStreamError::StreamConfigNotSupported)?;

        let sample_spec = make_sample_spec(config, format);
        let channel_map = make_channel_map(config);
        let buffer_attr = make_buffer_attr(config, format);

        let params = protocol::PlaybackStreamParams {
            sink_index: Some(info.index),
            sample_spec,
            channel_map,
            buffer_attr,
            flags: protocol::stream::StreamFlags {
                // Start the stream suspended.
                start_corked: true,
                ..Default::default()
            },
            ..Default::default()
        };

        stream::Stream::new_playback(client.clone(), params, data_callback, error_callback)
    }

    fn description(&self) -> Result<DeviceDescription, DeviceNameError> {
        let (name, description, direction) = match self {
            Device::Sink { info, .. } => (&info.name, &info.description, DeviceDirection::Output),
            Device::Source { info, .. } => (&info.name, &info.description, DeviceDirection::Input),
        };

        let mut builder = DeviceDescriptionBuilder::new(String::from_utf8_lossy(name.as_bytes()))
            .direction(direction);
        if let Some(desc) = description {
            builder = builder.add_extended_line(String::from_utf8_lossy(desc.as_bytes()));
        }

        Ok(builder.build())
    }

    fn id(&self) -> Result<DeviceId, DeviceIdError> {
        let id = match self {
            Device::Sink { info, .. } => info.index,
            Device::Source { info, .. } => info.index,
        };

        Ok(DeviceId(HostId::PulseAudio, id.to_string()))
    }
}

fn make_sample_spec(config: &StreamConfig, format: protocol::SampleFormat) -> protocol::SampleSpec {
    protocol::SampleSpec {
        format,
        sample_rate: config.sample_rate.0,
        channels: config.channels as _,
    }
}

fn make_channel_map(config: &StreamConfig) -> protocol::ChannelMap {
    if config.channels == 2 {
        return protocol::ChannelMap::stereo();
    }

    let mut map = protocol::ChannelMap::empty();
    for _ in 0..config.channels {
        map.push(protocol::ChannelPosition::Mono);
    }

    map
}

fn make_buffer_attr(
    config: &StreamConfig,
    format: protocol::SampleFormat,
) -> protocol::stream::BufferAttr {
    match config.buffer_size {
        crate::BufferSize::Default => Default::default(),
        crate::BufferSize::Fixed(frame_count) => {
            let len = frame_count * config.channels as u32 * format.bytes_per_sample() as u32;
            protocol::stream::BufferAttr {
                max_length: len,
                target_length: len,
                ..Default::default()
            }
        }
    }
}
