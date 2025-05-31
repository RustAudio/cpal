use std::cell::RefCell;
use std::cmp;
use std::convert::TryInto;
use std::time::{Duration, Instant};
use std::vec::IntoIter as VecIntoIter;

extern crate ndk;

use convert::{stream_instant, to_stream_instant};
use java_interface::{AudioDeviceDirection, AudioDeviceInfo};

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, InputStreamTimestamp, OutputCallbackInfo,
    OutputStreamTimestamp, PauseStreamError, PlayStreamError, SampleFormat, SampleRate,
    SizedSample, StreamConfig, StreamError, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange, SupportedStreamConfigsError,
};

mod android_media;
mod convert;
mod java_interface;

use self::android_media::{get_audio_record_min_buffer_size, get_audio_track_min_buffer_size};
use self::ndk::audio::AudioStream;

// Android Java API supports up to 8 channels
// TODO: more channels available in native AAudio
const CHANNEL_MASKS: [i32; 2] = [
    android_media::CHANNEL_OUT_MONO,
    android_media::CHANNEL_OUT_STEREO,
];

const SAMPLE_RATES: [i32; 13] = [
    5512, 8000, 11025, 16000, 22050, 32000, 44100, 48000, 64000, 88200, 96000, 176_400, 192_000,
];

pub struct Host;
#[derive(Clone)]
pub struct Device(Option<AudioDeviceInfo>);
pub enum Stream {
    Input(AudioStream),
    Output(AudioStream),
}

// AAudioStream is safe to be send, but not sync.
// See https://developer.android.com/ndk/guides/audio/aaudio/aaudio
// TODO: Is this still in-progress? https://github.com/rust-mobile/ndk/pull/497
unsafe impl Send for Stream {}

pub type SupportedInputConfigs = VecIntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = VecIntoIter<SupportedStreamConfigRange>;
pub type Devices = VecIntoIter<Device>;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        if let Ok(devices) = AudioDeviceInfo::request(AudioDeviceDirection::InputOutput) {
            Ok(devices
                .into_iter()
                .map(|d| Device(Some(d)))
                .collect::<Vec<_>>()
                .into_iter())
        } else {
            Ok(vec![Device(None)].into_iter())
        }
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        Some(Device(None))
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        Some(Device(None))
    }
}

fn buffer_size_range_for_params(
    is_output: bool,
    sample_rate: i32,
    channel_mask: i32,
    android_format: i32,
) -> SupportedBufferSize {
    let min_buffer_size = if is_output {
        get_audio_track_min_buffer_size(sample_rate, channel_mask, android_format)
    } else {
        get_audio_record_min_buffer_size(sample_rate, channel_mask, android_format)
    };
    if min_buffer_size > 0 {
        SupportedBufferSize::Range {
            min: min_buffer_size as u32,
            max: i32::MAX as u32,
        }
    } else {
        SupportedBufferSize::Unknown
    }
}

fn default_supported_configs(is_output: bool) -> VecIntoIter<SupportedStreamConfigRange> {
    // Have to "brute force" the parameter combinations with getMinBufferSize
    const FORMATS: [SampleFormat; 2] = [SampleFormat::I16, SampleFormat::F32];

    let mut output = Vec::with_capacity(SAMPLE_RATES.len() * CHANNEL_MASKS.len() * FORMATS.len());
    for sample_format in &FORMATS {
        let android_format = if *sample_format == SampleFormat::I16 {
            android_media::ENCODING_PCM_16BIT
        } else {
            android_media::ENCODING_PCM_FLOAT
        };
        for (mask_idx, channel_mask) in CHANNEL_MASKS.iter().enumerate() {
            let channel_count = mask_idx + 1;
            for sample_rate in &SAMPLE_RATES {
                if let SupportedBufferSize::Range { min, max } = buffer_size_range_for_params(
                    is_output,
                    *sample_rate,
                    *channel_mask,
                    android_format,
                ) {
                    output.push(SupportedStreamConfigRange {
                        channels: channel_count as u16,
                        min_sample_rate: SampleRate(*sample_rate as u32),
                        max_sample_rate: SampleRate(*sample_rate as u32),
                        buffer_size: SupportedBufferSize::Range { min, max },
                        sample_format: *sample_format,
                    });
                }
            }
        }
    }

    output.into_iter()
}

fn device_supported_configs(
    device: &AudioDeviceInfo,
    is_output: bool,
) -> VecIntoIter<SupportedStreamConfigRange> {
    let sample_rates = if !device.sample_rates.is_empty() {
        device.sample_rates.as_slice()
    } else {
        &SAMPLE_RATES
    };

    const ALL_CHANNELS: [i32; 2] = [1, 2];
    let channel_counts = if !device.channel_counts.is_empty() {
        device.channel_counts.as_slice()
    } else {
        &ALL_CHANNELS
    };

    const ALL_FORMATS: [SampleFormat; 2] = [SampleFormat::I16, SampleFormat::F32];
    let formats = if !device.formats.is_empty() {
        device.formats.as_slice()
    } else {
        &ALL_FORMATS
    };

    let mut output = Vec::with_capacity(sample_rates.len() * channel_counts.len() * formats.len());
    for sample_rate in sample_rates {
        for channel_count in channel_counts {
            assert!(*channel_count > 0);
            if *channel_count > 2 {
                // could be supported by the device
                // TODO: more channels available in native AAudio
                continue;
            }
            let channel_mask = CHANNEL_MASKS[*channel_count as usize - 1];
            for format in formats {
                let (android_format, sample_format) = match format {
                    SampleFormat::I16 => (android_media::ENCODING_PCM_16BIT, SampleFormat::I16),
                    SampleFormat::F32 => (android_media::ENCODING_PCM_FLOAT, SampleFormat::F32),
                    _ => panic!("Unexpected format"),
                };
                let buffer_size = buffer_size_range_for_params(
                    is_output,
                    *sample_rate,
                    channel_mask,
                    android_format,
                );
                output.push(SupportedStreamConfigRange {
                    channels: cmp::min(*channel_count as u16, 2u16),
                    min_sample_rate: SampleRate(*sample_rate as u32),
                    max_sample_rate: SampleRate(*sample_rate as u32),
                    buffer_size,
                    sample_format,
                });
            }
        }
    }

    output.into_iter()
}

fn configure_for_device(
    builder: ndk::audio::AudioStreamBuilder,
    device: &Device,
    config: &StreamConfig,
) -> ndk::audio::AudioStreamBuilder {
    let mut builder = if let Some(info) = &device.0 {
        builder.device_id(info.id)
    } else {
        builder
    };
    builder = builder.sample_rate(config.sample_rate.0.try_into().unwrap());
    match &config.buffer_size {
        BufferSize::Default => builder,
        BufferSize::Fixed(size) => builder.buffer_capacity_in_frames(*size as i32),
    }
}

fn build_input_stream<D, E>(
    device: &Device,
    config: &StreamConfig,
    mut data_callback: D,
    mut error_callback: E,
    builder: ndk::audio::AudioStreamBuilder,
    sample_format: SampleFormat,
) -> Result<Stream, BuildStreamError>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    let builder = configure_for_device(builder, device, config);
    let created = Instant::now();
    let channel_count = config.channels as i32;
    let stream = builder
        .data_callback(Box::new(move |stream, data, num_frames| {
            let cb_info = InputCallbackInfo {
                timestamp: InputStreamTimestamp {
                    callback: to_stream_instant(created.elapsed()),
                    capture: stream_instant(stream),
                },
            };
            (data_callback)(
                &unsafe {
                    Data::from_parts(
                        data as *mut _,
                        (num_frames * channel_count).try_into().unwrap(),
                        sample_format,
                    )
                },
                &cb_info,
            );
            ndk::audio::AudioCallbackResult::Continue
        }))
        .error_callback(Box::new(move |stream, error| {
            (error_callback)(StreamError::from(error))
        }))
        .open_stream()?;
    Ok(Stream::Input(stream))
}

fn build_output_stream<D, E>(
    device: &Device,
    config: &StreamConfig,
    mut data_callback: D,
    mut error_callback: E,
    builder: ndk::audio::AudioStreamBuilder,
    sample_format: SampleFormat,
) -> Result<Stream, BuildStreamError>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    let builder = configure_for_device(builder, device, config);
    let created = Instant::now();
    let channel_count = config.channels as i32;
    let stream = builder
        .data_callback(Box::new(move |stream, data, num_frames| {
            let cb_info = OutputCallbackInfo {
                timestamp: OutputStreamTimestamp {
                    callback: to_stream_instant(created.elapsed()),
                    playback: stream_instant(stream),
                },
            };
            (data_callback)(
                &mut unsafe {
                    Data::from_parts(
                        data as *mut _,
                        (num_frames * channel_count).try_into().unwrap(),
                        sample_format,
                    )
                },
                &cb_info,
            );
            ndk::audio::AudioCallbackResult::Continue
        }))
        .error_callback(Box::new(move |stream, error| {
            (error_callback)(StreamError::from(error))
        }))
        .open_stream()?;
    Ok(Stream::Output(stream))
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        match &self.0 {
            None => Ok("default".to_owned()),
            Some(info) => {
                let name = if info.address.is_empty() {
                    format!("{}:{:?}", info.product_name, info.device_type)
                } else {
                    format!(
                        "{}:{:?}:{}",
                        info.product_name, info.device_type, info.address
                    )
                };
                Ok(name)
            }
        }
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        if let Some(info) = &self.0 {
            Ok(device_supported_configs(info, false))
        } else {
            Ok(default_supported_configs(false))
        }
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        if let Some(info) = &self.0 {
            Ok(device_supported_configs(info, true))
        } else {
            Ok(default_supported_configs(true))
        }
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut configs: Vec<_> = self.supported_input_configs().unwrap().collect();
        configs.sort_by(|a, b| b.cmp_default_heuristics(a));
        let config = configs
            .into_iter()
            .next()
            .ok_or(DefaultStreamConfigError::StreamTypeNotSupported)?
            .with_max_sample_rate();
        Ok(config)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut configs: Vec<_> = self.supported_output_configs().unwrap().collect();
        configs.sort_by(|a, b| b.cmp_default_heuristics(a));
        let config = configs
            .into_iter()
            .next()
            .ok_or(DefaultStreamConfigError::StreamTypeNotSupported)?
            .with_max_sample_rate();
        Ok(config)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let format = match sample_format {
            SampleFormat::I16 => ndk::audio::AudioFormat::PCM_I16,
            SampleFormat::F32 => ndk::audio::AudioFormat::PCM_Float,
            sample_format => {
                return Err(BackendSpecificError {
                    description: format!("{} format is not supported on Android.", sample_format),
                }
                .into())
            }
        };
        let channel_count = match config.channels {
            1 => 1,
            2 => 2,
            channels => {
                // TODO: more channels available in native AAudio
                return Err(BackendSpecificError {
                    description: "More than 2 channels are not supported yet.".to_owned(),
                }
                .into());
            }
        };

        let builder = ndk::audio::AudioStreamBuilder::new()?
            .direction(ndk::audio::AudioDirection::Input)
            .channel_count(channel_count)
            .format(format);

        build_input_stream(
            self,
            config,
            data_callback,
            error_callback,
            builder,
            sample_format,
        )
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let format = match sample_format {
            SampleFormat::I16 => ndk::audio::AudioFormat::PCM_I16,
            SampleFormat::F32 => ndk::audio::AudioFormat::PCM_Float,
            sample_format => {
                return Err(BackendSpecificError {
                    description: format!("{} format is not supported on Android.", sample_format),
                }
                .into())
            }
        };
        let channel_count = match config.channels {
            1 => 1,
            2 => 2,
            channels => {
                // TODO: more channels available in native AAudio
                return Err(BackendSpecificError {
                    description: "More than 2 channels are not supported yet.".to_owned(),
                }
                .into());
            }
        };

        let builder = ndk::audio::AudioStreamBuilder::new()?
            .direction(ndk::audio::AudioDirection::Output)
            .channel_count(channel_count)
            .format(format);

        build_output_stream(
            self,
            config,
            data_callback,
            error_callback,
            builder,
            sample_format,
        )
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        match self {
            Self::Input(stream) => stream.request_start().map_err(PlayStreamError::from),
            Self::Output(stream) => stream.request_start().map_err(PlayStreamError::from),
        }
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        match self {
            Self::Input(_) => Err(BackendSpecificError {
                description: "Pause called on the input stream.".to_owned(),
            }
            .into()),
            Self::Output(stream) => stream.request_pause().map_err(PauseStreamError::from),
        }
    }
}
