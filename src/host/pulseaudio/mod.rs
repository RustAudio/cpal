use futures::executor::block_on;
use pulseaudio::protocol;

mod stream;

pub use stream::Stream;

use crate::{
    traits::{DeviceTrait, HostTrait},
    BackendSpecificError, BuildStreamError, Data, DefaultStreamConfigError, DeviceDescription,
    DeviceDescriptionBuilder, DeviceDirection, DeviceId, DeviceIdError, DeviceNameError,
    DevicesError, FrameCount, HostId, HostUnavailable, InputCallbackInfo, OutputCallbackInfo,
    SampleFormat, StreamConfig, StreamError, SupportedBufferSize, SupportedStreamConfig,
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

fn supported_config_ranges() -> Vec<SupportedStreamConfigRange> {
    let mut ranges = vec![];
    for format in PULSE_FORMATS {
        for channel_count in 1..protocol::sample_spec::MAX_CHANNELS {
            let bytes_per_frame = channel_count as usize * format.sample_size();
            let max_frames = (protocol::MAX_MEMBLOCKQ_LENGTH / bytes_per_frame) as FrameCount;
            ranges.push(SupportedStreamConfigRange {
                channels: channel_count as _,
                min_sample_rate: 1,
                max_sample_rate: protocol::sample_spec::MAX_RATE,
                buffer_size: SupportedBufferSize::Range {
                    min: 0,
                    max: max_frames,
                },
                sample_format: *format,
            });
        }
    }
    ranges
}

fn default_config_from_spec(
    sample_spec: &protocol::SampleSpec,
    channel_map: &protocol::ChannelMap,
) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
    let sample_format: SampleFormat = sample_spec
        .format
        .try_into()
        .map_err(|_| DefaultStreamConfigError::StreamTypeNotSupported)?;
    let bytes_per_frame = channel_map.num_channels() as usize * sample_format.sample_size();
    let max_frames = (protocol::MAX_MEMBLOCKQ_LENGTH / bytes_per_frame) as u32;
    Ok(SupportedStreamConfig {
        channels: channel_map.num_channels() as _,
        sample_rate: sample_spec.sample_rate,
        buffer_size: SupportedBufferSize::Range {
            min: 0,
            max: max_frames,
        },
        sample_format,
    })
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
        Ok(supported_config_ranges().into_iter())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        let Device::Sink { .. } = self else {
            return Ok(vec![].into_iter());
        };
        Ok(supported_config_ranges().into_iter())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let Device::Source { info, .. } = self else {
            return Err(DefaultStreamConfigError::StreamTypeNotSupported);
        };
        default_config_from_spec(&info.sample_spec, &info.channel_map)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let Device::Sink { info, .. } = self else {
            return Err(DefaultStreamConfigError::StreamTypeNotSupported);
        };
        default_config_from_spec(&info.sample_spec, &info.channel_map)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
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
        let buffer_attr = make_record_buffer_attr(config, format);
        let adjust_latency = matches!(config.buffer_size, crate::BufferSize::Fixed(_));

        let params = protocol::RecordStreamParams {
            sample_spec,
            channel_map,
            source_index: Some(info.index),
            buffer_attr,
            flags: protocol::stream::StreamFlags {
                // Start the stream suspended.
                start_corked: true,
                // When a fixed buffer size is requested, ask PA to configure
                // the source hardware to hit the requested latency end-to-end.
                adjust_latency,
                ..Default::default()
            },
            ..Default::default()
        };

        stream::Stream::new_record(client.clone(), params, data_callback, error_callback)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
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
        let buffer_attr = make_playback_buffer_attr(config, format);
        let adjust_latency = matches!(config.buffer_size, crate::BufferSize::Fixed(_));

        let params = protocol::PlaybackStreamParams {
            sink_index: Some(info.index),
            sample_spec,
            channel_map,
            buffer_attr,
            flags: protocol::stream::StreamFlags {
                // Start the stream suspended.
                start_corked: true,
                // When a fixed buffer size is requested, ask PA to configure
                // the sink hardware to hit the requested latency end-to-end.
                adjust_latency,
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

fn make_sample_spec(config: StreamConfig, format: protocol::SampleFormat) -> protocol::SampleSpec {
    protocol::SampleSpec {
        format,
        sample_rate: config.sample_rate,
        channels: config.channels as _,
    }
}

fn make_channel_map(config: StreamConfig) -> protocol::ChannelMap {
    use protocol::ChannelPosition::*;

    // Standard channel layouts following the PulseAudio default channel map
    // (PA_CHANNEL_MAP_DEFAULT) for 1-8 channels, and common Atmos height-
    // channel conventions for 10 and 12 channels. Counts without a widely
    // agreed layout (9, 11, >12) fall back to sequential Aux positions.
    let standard: &[protocol::ChannelPosition] = match config.channels {
        1 => &[Mono],
        2 => &[FrontLeft, FrontRight],
        3 => &[FrontLeft, FrontRight, FrontCenter],
        4 => &[FrontLeft, FrontRight, RearLeft, RearRight],
        5 => &[FrontLeft, FrontRight, FrontCenter, RearLeft, RearRight],
        6 => &[FrontLeft, FrontRight, FrontCenter, Lfe, RearLeft, RearRight],
        7 => &[
            FrontLeft,
            FrontRight,
            FrontCenter,
            Lfe,
            RearLeft,
            RearRight,
            RearCenter,
        ],
        8 => &[
            FrontLeft,
            FrontRight,
            FrontCenter,
            Lfe,
            RearLeft,
            RearRight,
            SideLeft,
            SideRight,
        ],
        // 7.1.2 (Dolby Atmos): 7.1 + top-front L/R
        10 => &[
            FrontLeft,
            FrontRight,
            FrontCenter,
            Lfe,
            RearLeft,
            RearRight,
            SideLeft,
            SideRight,
            TopFrontLeft,
            TopFrontRight,
        ],
        // 7.1.4 (Dolby Atmos): 7.1 + top-front L/R + top-rear L/R
        12 => &[
            FrontLeft,
            FrontRight,
            FrontCenter,
            Lfe,
            RearLeft,
            RearRight,
            SideLeft,
            SideRight,
            TopFrontLeft,
            TopFrontRight,
            TopRearLeft,
            TopRearRight,
        ],
        _ => &[],
    };

    if !standard.is_empty() {
        return protocol::ChannelMap::new(standard.iter().copied());
    }

    let aux = [
        Aux0, Aux1, Aux2, Aux3, Aux4, Aux5, Aux6, Aux7, Aux8, Aux9, Aux10, Aux11, Aux12, Aux13,
        Aux14, Aux15, Aux16, Aux17, Aux18, Aux19, Aux20, Aux21, Aux22, Aux23, Aux24, Aux25, Aux26,
        Aux27, Aux28, Aux29, Aux30, Aux31,
    ];
    protocol::ChannelMap::new(aux.iter().copied().take(config.channels as usize))
}

fn make_playback_buffer_attr(
    config: StreamConfig,
    format: protocol::SampleFormat,
) -> protocol::stream::BufferAttr {
    match config.buffer_size {
        crate::BufferSize::Default => Default::default(),
        crate::BufferSize::Fixed(frame_count) => {
            let len = frame_count * config.channels as u32 * format.bytes_per_sample() as u32;
            protocol::stream::BufferAttr {
                // Double-buffer: total buffer = 2 callback periods. With
                // adjust_latency this becomes the end-to-end latency target,
                // Minimum request = one callback period, ensuring the server
                // always asks for exactly frame_count frames per call.
                max_length: 2 * len,
                target_length: 2 * len,
                minimum_request_length: len,
                ..Default::default()
            }
        }
    }
}

fn make_record_buffer_attr(
    config: StreamConfig,
    format: protocol::SampleFormat,
) -> protocol::stream::BufferAttr {
    match config.buffer_size {
        crate::BufferSize::Default => Default::default(),
        crate::BufferSize::Fixed(frame_count) => {
            let len = frame_count * config.channels as u32 * format.bytes_per_sample() as u32;
            protocol::stream::BufferAttr {
                // fragment_size controls the delivery chunk size for record
                // streams; target_length is playback-only and is ignored here.
                max_length: len,
                fragment_size: len,
                ..Default::default()
            }
        }
    }
}
