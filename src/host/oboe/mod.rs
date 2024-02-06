use std::cell::RefCell;
use std::cmp;
use std::convert::TryInto;
use std::time::Duration;
use std::vec::IntoIter as VecIntoIter;

extern crate oboe;

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, SizedSample, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

mod android_media;
mod convert;
mod input_callback;
mod output_callback;

use self::android_media::{get_audio_record_min_buffer_size, get_audio_track_min_buffer_size};
use self::input_callback::CpalInputCallback;
use self::oboe::{AudioInputStream, AudioOutputStream};
use self::output_callback::CpalOutputCallback;

// Android Java API supports up to 8 channels, but oboe API
// only exposes mono and stereo.
const CHANNEL_MASKS: [i32; 2] = [
    android_media::CHANNEL_OUT_MONO,
    android_media::CHANNEL_OUT_STEREO,
];

const SAMPLE_RATES: [i32; 13] = [
    5512, 8000, 11025, 16000, 22050, 32000, 44100, 48000, 64000, 88200, 96000, 176_400, 192_000,
];

pub struct Host;
#[derive(Clone)]
pub struct Device(Option<oboe::AudioDeviceInfo>);
pub enum Stream {
    Input(Box<RefCell<dyn AudioInputStream>>),
    Output(Box<RefCell<dyn AudioOutputStream>>),
}
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
        if let Ok(devices) = oboe::AudioDeviceInfo::request(oboe::AudioDeviceDirection::InputOutput)
        {
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
    device: &oboe::AudioDeviceInfo,
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

    const ALL_FORMATS: [oboe::AudioFormat; 2] = [oboe::AudioFormat::I16, oboe::AudioFormat::F32];
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
                // could be supported by the device, but oboe does not support more than 2 channels
                continue;
            }
            let channel_mask = CHANNEL_MASKS[*channel_count as usize - 1];
            for format in formats {
                let (android_format, sample_format) = match format {
                    oboe::AudioFormat::I16 => {
                        (android_media::ENCODING_PCM_16BIT, SampleFormat::I16)
                    }
                    oboe::AudioFormat::F32 => {
                        (android_media::ENCODING_PCM_FLOAT, SampleFormat::F32)
                    }
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

fn configure_for_device<D, C, I>(
    builder: oboe::AudioStreamBuilder<D, C, I>,
    device: &Device,
    config: &StreamConfig,
) -> oboe::AudioStreamBuilder<D, C, I> {
    let mut builder = if let Some(info) = &device.0 {
        builder.set_device_id(info.id)
    } else {
        builder
    };
    builder = builder.set_sample_rate(config.sample_rate.0.try_into().unwrap());
    match &config.buffer_size {
        BufferSize::Default => builder,
        BufferSize::Fixed(size) => builder.set_buffer_capacity_in_frames(*size as i32),
    }
}

fn build_input_stream<D, E, C, T>(
    device: &Device,
    config: &StreamConfig,
    data_callback: D,
    error_callback: E,
    builder: oboe::AudioStreamBuilder<oboe::Input, C, T>,
) -> Result<Stream, BuildStreamError>
where
    T: SizedSample + oboe::IsFormat + Send + 'static,
    C: oboe::IsChannelCount + Send + 'static,
    (T, C): oboe::IsFrameType,
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    let builder = configure_for_device(builder, device, config);
    let stream = builder
        .set_callback(CpalInputCallback::<T, C>::new(
            data_callback,
            error_callback,
        ))
        .open_stream()?;
    Ok(Stream::Input(Box::new(RefCell::new(stream))))
}

fn build_output_stream<D, E, C, T>(
    device: &Device,
    config: &StreamConfig,
    data_callback: D,
    error_callback: E,
    builder: oboe::AudioStreamBuilder<oboe::Output, C, T>,
) -> Result<Stream, BuildStreamError>
where
    T: SizedSample + oboe::IsFormat + Send + 'static,
    C: oboe::IsChannelCount + Send + 'static,
    (T, C): oboe::IsFrameType,
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(StreamError) + Send + 'static,
{
    let builder = configure_for_device(builder, device, config);
    let stream = builder
        .set_callback(CpalOutputCallback::<T, C>::new(
            data_callback,
            error_callback,
        ))
        .open_stream()?;
    Ok(Stream::Output(Box::new(RefCell::new(stream))))
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        match &self.0 {
            None => Ok("default".to_owned()),
            Some(info) => Ok(info.product_name.clone()),
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
        match sample_format {
            SampleFormat::I16 => {
                let builder = oboe::AudioStreamBuilder::default()
                    .set_input()
                    .set_format::<i16>();
                if config.channels == 1 {
                    build_input_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_mono(),
                    )
                } else if config.channels == 2 {
                    build_input_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_stereo(),
                    )
                } else {
                    Err(BackendSpecificError {
                        description: "More than 2 channels are not supported by Oboe.".to_owned(),
                    }
                    .into())
                }
            }
            SampleFormat::F32 => {
                let builder = oboe::AudioStreamBuilder::default()
                    .set_input()
                    .set_format::<f32>();
                if config.channels == 1 {
                    build_input_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_mono(),
                    )
                } else if config.channels == 2 {
                    build_input_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_stereo(),
                    )
                } else {
                    Err(BackendSpecificError {
                        description: "More than 2 channels are not supported by Oboe.".to_owned(),
                    }
                    .into())
                }
            }
            sample_format => Err(BackendSpecificError {
                description: format!("{} format is not supported on Android.", sample_format),
            }
            .into()),
        }
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
        match sample_format {
            SampleFormat::I16 => {
                let builder = oboe::AudioStreamBuilder::default()
                    .set_output()
                    .set_format::<i16>();
                if config.channels == 1 {
                    build_output_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_mono(),
                    )
                } else if config.channels == 2 {
                    build_output_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_stereo(),
                    )
                } else {
                    Err(BackendSpecificError {
                        description: "More than 2 channels are not supported by Oboe.".to_owned(),
                    }
                    .into())
                }
            }
            SampleFormat::F32 => {
                let builder = oboe::AudioStreamBuilder::default()
                    .set_output()
                    .set_format::<f32>();
                if config.channels == 1 {
                    build_output_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_mono(),
                    )
                } else if config.channels == 2 {
                    build_output_stream(
                        self,
                        config,
                        data_callback,
                        error_callback,
                        builder.set_stereo(),
                    )
                } else {
                    Err(BackendSpecificError {
                        description: "More than 2 channels are not supported by Oboe.".to_owned(),
                    }
                    .into())
                }
            }
            sample_format => Err(BackendSpecificError {
                description: format!("{} format is not supported on Android.", sample_format),
            }
            .into()),
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        match self {
            Self::Input(stream) => stream
                .borrow_mut()
                .request_start()
                .map_err(PlayStreamError::from),
            Self::Output(stream) => stream
                .borrow_mut()
                .request_start()
                .map_err(PlayStreamError::from),
        }
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        match self {
            Self::Input(_) => Err(BackendSpecificError {
                description: "Pause called on the input stream.".to_owned(),
            }
            .into()),
            Self::Output(stream) => stream
                .borrow_mut()
                .request_pause()
                .map_err(PauseStreamError::from),
        }
    }
}
