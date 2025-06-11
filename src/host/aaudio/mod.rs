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

/// Audio usage types for Android AAudio streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioUsage {
    /// Media playback (default) - routes to speakers
    Media,
    /// Voice communication - routes to earpiece/headset for VoIP
    VoiceCommunication,
    /// Traditional phone calls - routes to earpiece/headset
    VoiceCall,
    /// Voice recognition - optimized for speech input
    VoiceRecognition,
    /// Games and interactive applications
    Game,
    /// Assistant applications
    Assistant,
}

impl AudioUsage {
    fn to_ndk_usage(self) -> ndk::audio::AudioUsage {
        match self {
            AudioUsage::Media => ndk::audio::AudioUsage::Media,
            AudioUsage::VoiceCommunication => ndk::audio::AudioUsage::VoiceCommunication,
            AudioUsage::VoiceCall => ndk::audio::AudioUsage::VoiceCall,
            AudioUsage::VoiceRecognition => ndk::audio::AudioUsage::VoiceRecognition,
            AudioUsage::Game => ndk::audio::AudioUsage::Game,
            AudioUsage::Assistant => ndk::audio::AudioUsage::Assistant,
        }
    }
}

impl Default for AudioUsage {
    fn default() -> Self {
        AudioUsage::Media
    }
}

/// Audio content types for Android AAudio streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioContentType {
    /// Music content
    Music,
    /// Speech content for VoIP/calls
    Speech,
    /// Movie content
    Movie,
    /// Sonification (UI sounds, notifications)
    Sonification,
}

impl AudioContentType {
    fn to_ndk_content_type(self) -> ndk::audio::AudioContentType {
        match self {
            AudioContentType::Music => ndk::audio::AudioContentType::Music,
            AudioContentType::Speech => ndk::audio::AudioContentType::Speech,
            AudioContentType::Movie => ndk::audio::AudioContentType::Movie,
            AudioContentType::Sonification => ndk::audio::AudioContentType::Sonification,
        }
    }
}

impl Default for AudioContentType {
    fn default() -> Self {
        AudioContentType::Music
    }
}

/// Audio input presets for Android AAudio streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioInputPreset {
    /// Voice recognition (default, lowest latency)
    VoiceRecognition,
    /// Voice communication for VoIP
    VoiceCommunication,
    /// High quality voice recording
    VoicePerformance,
    /// General audio recording
    Generic,
    /// Camera/camcorder recording
    Camcorder,
}

impl AudioInputPreset {
    fn to_ndk_input_preset(self) -> ndk::audio::AudioInputPreset {
        match self {
            AudioInputPreset::VoiceRecognition => ndk::audio::AudioInputPreset::VoiceRecognition,
            AudioInputPreset::VoiceCommunication => ndk::audio::AudioInputPreset::VoiceCommunication,
            AudioInputPreset::VoicePerformance => ndk::audio::AudioInputPreset::VoicePerformance,
            AudioInputPreset::Generic => ndk::audio::AudioInputPreset::Generic,
            AudioInputPreset::Camcorder => ndk::audio::AudioInputPreset::Camcorder,
        }
    }
}

impl Default for AudioInputPreset {
    fn default() -> Self {
        AudioInputPreset::VoiceRecognition
    }
}

/// Audio sharing modes for Android AAudio streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSharingMode {
    /// Shared mode - allows other apps to use audio (recommended)
    Shared,
    /// Exclusive mode - blocks other apps from using audio (not recommended)
    Exclusive,
}

impl AudioSharingMode {
    fn to_ndk_sharing_mode(self) -> ndk::audio::AudioSharingMode {
        match self {
            AudioSharingMode::Shared => ndk::audio::AudioSharingMode::Shared,
            AudioSharingMode::Exclusive => ndk::audio::AudioSharingMode::Exclusive,
        }
    }
}

impl Default for AudioSharingMode {
    fn default() -> Self {
        AudioSharingMode::Shared
    }
}

/// Audio performance modes for Android AAudio streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPerformanceMode {
    /// Default mode - balanced latency and power
    None,
    /// Low latency mode for real-time applications
    LowLatency,
    /// Power saving mode for longer battery life
    PowerSaving,
}

impl AudioPerformanceMode {
    fn to_ndk_performance_mode(self) -> ndk::audio::AudioPerformanceMode {
        match self {
            AudioPerformanceMode::None => ndk::audio::AudioPerformanceMode::None,
            AudioPerformanceMode::LowLatency => ndk::audio::AudioPerformanceMode::LowLatency,
            AudioPerformanceMode::PowerSaving => ndk::audio::AudioPerformanceMode::PowerSaving,
        }
    }
}

impl Default for AudioPerformanceMode {
    fn default() -> Self {
        AudioPerformanceMode::None
    }
}

/// Extended stream configuration for Android-specific audio settings
#[derive(Debug, Clone)]
pub struct AndroidStreamConfig {
    /// Standard stream configuration
    pub base: StreamConfig,
    /// Audio usage type (affects routing)
    pub usage: AudioUsage,
    /// Audio content type
    pub content_type: AudioContentType,
    /// Input preset (for input streams only)
    pub input_preset: AudioInputPreset,
    /// Sharing mode
    pub sharing_mode: AudioSharingMode,
    /// Performance mode
    pub performance_mode: AudioPerformanceMode,
}

impl AndroidStreamConfig {
    pub fn new(base: StreamConfig) -> Self {
        Self {
            base,
            usage: AudioUsage::default(),
            content_type: AudioContentType::default(),
            input_preset: AudioInputPreset::default(),
            sharing_mode: AudioSharingMode::default(),
            performance_mode: AudioPerformanceMode::default(),
        }
    }

    /// Create configuration optimized for VoIP applications
    pub fn for_voip(base: StreamConfig) -> Self {
        Self {
            base,
            usage: AudioUsage::VoiceCommunication,
            content_type: AudioContentType::Speech,
            input_preset: AudioInputPreset::VoiceCommunication,
            sharing_mode: AudioSharingMode::Shared, // Always shared for VoIP
            performance_mode: AudioPerformanceMode::LowLatency,
        }
    }

    /// Create configuration optimized for traditional phone calls
    pub fn for_voice_call(base: StreamConfig) -> Self {
        Self {
            base,
            usage: AudioUsage::VoiceCall,
            content_type: AudioContentType::Speech,
            input_preset: AudioInputPreset::VoiceCommunication,
            sharing_mode: AudioSharingMode::Shared,
            performance_mode: AudioPerformanceMode::LowLatency,
        }
    }

    /// Create configuration for media playback
    pub fn for_media(base: StreamConfig) -> Self {
        Self {
            base,
            usage: AudioUsage::Media,
            content_type: AudioContentType::Music,
            input_preset: AudioInputPreset::Generic,
            sharing_mode: AudioSharingMode::Shared,
            performance_mode: AudioPerformanceMode::None,
        }
    }
}

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
    config: &AndroidStreamConfig,
) -> ndk::audio::AudioStreamBuilder {
    let mut builder = if let Some(info) = &device.0 {
        builder.device_id(info.id)
    } else {
        builder
    };
    builder = builder.sample_rate(config.base.sample_rate.0.try_into().unwrap());
    
    builder = builder
        .sharing_mode(config.sharing_mode.to_ndk_sharing_mode())
        .performance_mode(config.performance_mode.to_ndk_performance_mode());

    // Apply usage and content type (API level 28+)
    #[cfg(feature = "api-level-28")]
    {
        builder = builder
            .usage(config.usage.to_ndk_usage())
            .content_type(config.content_type.to_ndk_content_type());
    }

    match &config.base.buffer_size {
        BufferSize::Default => builder,
        BufferSize::Fixed(size) => builder.buffer_capacity_in_frames(*size as i32),
    }
}

fn build_input_stream<D, E>(
    device: &Device,
    config: &AndroidStreamConfig,
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

    // Apply input preset (API level 28+)
    #[cfg(feature = "api-level-28")]
    {
        builder = builder.input_preset(config.input_preset.to_ndk_input_preset());
    }

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
    config: &AndroidStreamConfig,
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

impl Device {
    /// Build input stream with Android-specific configuration
    pub fn build_input_stream_with_android_config<D, E>(
        &self,
        config: &AndroidStreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>
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
        let channel_count = match config.base.channels {
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

    /// Build output stream with Android-specific configuration
    pub fn build_output_stream_with_android_config<D, E>(
        &self,
        config: &AndroidStreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Stream, BuildStreamError>
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
        let channel_count = match config.base.channels {
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
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Use default Android configuration (media usage)
        let android_config = AndroidStreamConfig::for_media(config.clone());
        self.build_input_stream_with_android_config(
            &android_config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
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
        // Use default Android configuration (media usage)
        let android_config = AndroidStreamConfig::for_media(config.clone());
        self.build_output_stream_with_android_config(
            &android_config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
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
