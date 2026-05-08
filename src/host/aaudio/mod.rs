//! AAudio backend implementation.
//!
//! Default backend on Android.

use std::{
    cmp,
    convert::TryInto,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
    vec::IntoIter as VecIntoIter,
};

use crate::{
    host::{emit_error, ErrorCallbackArc},
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceDirection,
    DeviceId, DeviceType, Error, ErrorKind, FrameCount, InputCallbackInfo, InputStreamTimestamp,
    InterfaceType, OutputCallbackInfo, OutputStreamTimestamp, ResultExt, SampleFormat, SampleRate,
    StreamConfig, StreamInstant, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
};

extern crate ndk;
use self::ndk::audio::AudioStream;
#[cfg(feature = "realtime")]
use crate::host::try_emit_error;

mod convert;
mod java_interface;

use convert::{input_stream_instant, now_stream_instant, output_stream_instant};
use java_interface::{AudioDeviceInfo, AudioDeviceType as AndroidDeviceType, AudioManager};

impl From<AndroidDeviceType> for DeviceType {
    fn from(device_type: AndroidDeviceType) -> Self {
        match device_type {
            AndroidDeviceType::BuiltinSpeaker
            | AndroidDeviceType::BuiltinSpeakerSafe
            | AndroidDeviceType::BleSpeaker => DeviceType::Speaker,

            AndroidDeviceType::BuiltinMic => DeviceType::Microphone,

            AndroidDeviceType::WiredHeadphones => DeviceType::Headphones,

            AndroidDeviceType::WiredHeadset
            | AndroidDeviceType::UsbHeadset
            | AndroidDeviceType::BleHeadset
            | AndroidDeviceType::BluetoothSCO => DeviceType::Headset,

            AndroidDeviceType::BuiltinEarpiece => DeviceType::Earpiece,

            AndroidDeviceType::HearingAid => DeviceType::HearingAid,

            AndroidDeviceType::Dock => DeviceType::Dock,

            AndroidDeviceType::Fm | AndroidDeviceType::FmTuner | AndroidDeviceType::TvTuner => {
                DeviceType::Tuner
            }

            AndroidDeviceType::RemoteSubmix => DeviceType::Virtual,

            _ => DeviceType::Unknown,
        }
    }
}

impl From<AndroidDeviceType> for InterfaceType {
    fn from(device_type: AndroidDeviceType) -> Self {
        match device_type {
            AndroidDeviceType::UsbDevice
            | AndroidDeviceType::UsbAccessory
            | AndroidDeviceType::UsbHeadset => InterfaceType::Usb,

            AndroidDeviceType::BluetoothA2DP
            | AndroidDeviceType::BluetoothSCO
            | AndroidDeviceType::BleHeadset
            | AndroidDeviceType::BleSpeaker
            | AndroidDeviceType::BleBroadcast => InterfaceType::Bluetooth,

            AndroidDeviceType::Hdmi | AndroidDeviceType::HdmiArc | AndroidDeviceType::HdmiEarc => {
                InterfaceType::Hdmi
            }

            AndroidDeviceType::LineAnalog
            | AndroidDeviceType::LineDigital
            | AndroidDeviceType::AuxLine => InterfaceType::Line,

            AndroidDeviceType::BuiltinEarpiece
            | AndroidDeviceType::BuiltinMic
            | AndroidDeviceType::BuiltinSpeaker
            | AndroidDeviceType::BuiltinSpeakerSafe => InterfaceType::BuiltIn,

            AndroidDeviceType::Ip => InterfaceType::Network,

            AndroidDeviceType::RemoteSubmix => InterfaceType::Virtual,

            _ => InterfaceType::Unknown,
        }
    }
}

// constants from android.media.AudioFormat
const CHANNEL_OUT_MONO: i32 = 4;
const CHANNEL_OUT_STEREO: i32 = 12;

// Android Java API supports up to 8 channels
// TODO: more channels available in native AAudio
// Maps channel masks to their corresponding channel counts
const CHANNEL_CONFIGS: [(i32, ChannelCount); 2] = [(CHANNEL_OUT_MONO, 1), (CHANNEL_OUT_STEREO, 2)];

const SAMPLE_RATES: [i32; 15] = [
    5512, 8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000,
    176_400, 192_000,
];

/// The same default for blocking operations as Oboe uses
const DEFAULT_TIMEOUT_NANOS: i64 = 2_000_000_000;

pub struct Host;
#[derive(Clone)]
pub struct Device(Option<AudioDeviceInfo>);

/// Stream wraps AudioStream in Arc<Mutex<>> to provide Send + Sync semantics.
///
/// While the underlying ndk::audio::AudioStream is neither Send nor Sync in ndk 0.9.0
/// (see https://developer.android.com/ndk/guides/audio/aaudio/aaudio#thread-safety),
/// we wrap it in a mutex to enable safe concurrent access and manually implement Send + Sync.
///
/// # Safety
///
/// This is safe because:
/// - AAudio functions are designed to be called from any thread (the Android docs state
///   "AAudio is not thread-safe" meaning it lacks internal locking, not that it's unsafe)
/// - Audio callbacks are called on a dedicated AAudio thread and don't access Stream
/// - The Mutex ensures exclusive access for control operations (play, pause)
/// - The pointer in AudioStream (NonNull<AAudioStreamStruct>) is valid for the lifetime
///   of the stream and AAudio C API functions are thread-safe at the C level
#[derive(Clone)]
pub struct Stream {
    inner: Arc<Mutex<AudioStream>>,
    direction: DeviceDirection,
}

// SAFETY: AudioStream can be safely sent between threads. The AAudio C API is thread-safe
// for moving stream ownership between threads. The NonNull pointer remains valid.
unsafe impl Send for Stream {}

// SAFETY: AudioStream can be safely shared between threads when protected by a Mutex.
// All operations on the stream go through the mutex, ensuring exclusive access.
unsafe impl Sync for Stream {}

// Compile-time assertion that Stream is Send and Sync
crate::assert_stream_send!(Stream);
crate::assert_stream_sync!(Stream);

/// State for dynamic buffer tuning on output streams.
#[derive(Default)]
struct BufferTuningState {
    previous_underrun_count: AtomicI32,
    capacity: AtomicI32,
    mixer_bursts: AtomicI32,
}

pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};
pub type Devices = std::vec::IntoIter<Device>;

impl Host {
    pub fn new() -> Result<Self, Error> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        if let Ok(devices) = AudioDeviceInfo::request(DeviceDirection::Duplex) {
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

fn buffer_size_range() -> SupportedBufferSize {
    SupportedBufferSize::Range {
        min: 1,
        max: i32::MAX as FrameCount,
    }
}

fn default_supported_configs() -> VecIntoIter<SupportedStreamConfigRange> {
    const FORMATS: [SampleFormat; 2] = [SampleFormat::I16, SampleFormat::F32];

    let buffer_size = buffer_size_range();
    let mut output = Vec::with_capacity(SAMPLE_RATES.len() * CHANNEL_CONFIGS.len() * FORMATS.len());
    for sample_format in &FORMATS {
        for (_channel_mask, channel_count) in &CHANNEL_CONFIGS {
            for sample_rate in &SAMPLE_RATES {
                output.push(SupportedStreamConfigRange {
                    channels: *channel_count,
                    min_sample_rate: *sample_rate as SampleRate,
                    max_sample_rate: *sample_rate as SampleRate,
                    buffer_size,
                    sample_format: *sample_format,
                });
            }
        }
    }

    output.into_iter()
}

fn device_supported_configs(device: &AudioDeviceInfo) -> VecIntoIter<SupportedStreamConfigRange> {
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

    let buffer_size = buffer_size_range();
    let mut output = Vec::with_capacity(sample_rates.len() * channel_counts.len() * formats.len());
    for sample_rate in sample_rates {
        for channel_count in channel_counts {
            assert!(*channel_count > 0);
            if *channel_count > 2 {
                // could be supported by the device
                // TODO: more channels available in native AAudio
                continue;
            }
            for format in formats {
                output.push(SupportedStreamConfigRange {
                    channels: cmp::min(*channel_count as ChannelCount, 2),
                    min_sample_rate: *sample_rate as SampleRate,
                    max_sample_rate: *sample_rate as SampleRate,
                    buffer_size,
                    sample_format: *format,
                });
            }
        }
    }

    output.into_iter()
}

fn configure_for_device(
    builder: ndk::audio::AudioStreamBuilder,
    device: &Device,
    config: StreamConfig,
) -> ndk::audio::AudioStreamBuilder {
    let mut builder = if let Some(info) = &device.0 {
        builder.device_id(info.id)
    } else {
        builder
    };
    builder = builder.sample_rate(config.sample_rate.try_into().unwrap());

    // Following the pattern from Oboe and Google's AAudio, we let AAudio choose the optimal
    // callback size dynamically by default. See
    // - https://developer.android.com/ndk/reference/group/audio#aaudiostreambuilder_setframesperdatacallback
    // - https://developer.android.com/ndk/guides/audio/audio-latency#buffer-size
    if let BufferSize::Fixed(size) = config.buffer_size {
        // For fixed sizes, the user explicitly wants control over the callback size.
        builder = builder
            .frames_per_data_callback(size.min(i32::MAX as FrameCount) as i32)
            .buffer_capacity_in_frames(size.saturating_mul(2).min(i32::MAX as FrameCount) as i32);
    }

    #[cfg(feature = "realtime")]
    {
        builder = builder.performance_mode(ndk::audio::AudioPerformanceMode::LowLatency);
    }

    builder
}

fn build_input_stream<D, E>(
    device: &Device,
    config: StreamConfig,
    mut data_callback: D,
    error_callback: E,
    builder: ndk::audio::AudioStreamBuilder,
    sample_format: SampleFormat,
) -> Result<Stream, Error>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    E: FnMut(Error) + Send + 'static,
{
    let builder = configure_for_device(builder, device, config);
    let channel_count = config.channels as i32;
    let sample_rate = config.sample_rate;

    let error_callback: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));
    let error_callback_for_stream = error_callback.clone();

    // RT check: run once on the first callback invocation to avoid delivering RealtimeDenied
    // before the Stream handle is returned to the caller.
    #[cfg(feature = "realtime")]
    let mut rt_checked = false;
    #[cfg(feature = "realtime")]
    let error_callback_for_rt = error_callback.clone();

    let stream = builder
        .data_callback(Box::new(move |stream, data, num_frames| {
            #[cfg(feature = "realtime")]
            if !rt_checked {
                if stream.performance_mode() != ndk::audio::AudioPerformanceMode::LowLatency {
                    if try_emit_error(
                        &error_callback_for_rt,
                        Error::new(ErrorKind::RealtimeDenied),
                    )
                    .is_ok()
                    {
                        rt_checked = true;
                    }
                } else {
                    rt_checked = true;
                }
            }

            let cb_info = InputCallbackInfo {
                timestamp: InputStreamTimestamp {
                    callback: now_stream_instant(),
                    capture: input_stream_instant(stream, sample_rate),
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
        .error_callback(Box::new(move |_stream, error| {
            emit_error(&error_callback_for_stream, Error::from(error));
        }))
        .open_stream()?;

    // SAFETY: Stream implements Send + Sync (see unsafe impl below). Arc<Mutex<AudioStream>>
    // is safe because the Mutex provides exclusive access and AudioStream's thread safety
    // is documented in the AAudio C API.
    #[allow(clippy::arc_with_non_send_sync)]
    Ok(Stream {
        inner: Arc::new(Mutex::new(stream)),
        direction: DeviceDirection::Input,
    })
}

fn build_output_stream<D, E>(
    device: &Device,
    config: StreamConfig,
    mut data_callback: D,
    error_callback: E,
    builder: ndk::audio::AudioStreamBuilder,
    sample_format: SampleFormat,
) -> Result<Stream, Error>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    E: FnMut(Error) + Send + 'static,
{
    let builder = configure_for_device(builder, device, config);
    let channel_count = config.channels as i32;
    let sample_rate = config.sample_rate;
    let tune_dynamically = config.buffer_size == BufferSize::Default;

    let tuning = Arc::new(BufferTuningState::default());
    let tuning_for_callback = tuning.clone();

    let error_callback: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));
    let error_callback_for_stream = error_callback.clone();

    // RT check: run once on the first callback invocation to avoid delivering RealtimeDenied
    // before the Stream handle is returned to the caller.
    #[cfg(feature = "realtime")]
    let mut rt_checked = false;
    #[cfg(feature = "realtime")]
    let error_callback_for_rt = error_callback.clone();

    let stream = builder
        .data_callback(Box::new(move |stream, data, num_frames| {
            #[cfg(feature = "realtime")]
            if !rt_checked {
                if stream.performance_mode() != ndk::audio::AudioPerformanceMode::LowLatency {
                    if try_emit_error(
                        &error_callback_for_rt,
                        Error::new(ErrorKind::RealtimeDenied),
                    )
                    .is_ok()
                    {
                        rt_checked = true;
                    }
                } else {
                    rt_checked = true;
                }
            }

            // Pre-fill with equilibrium so unwritten frames are silent.
            let n_samples: usize = (num_frames * channel_count).try_into().unwrap();
            let byte_count = n_samples * sample_format.sample_size();
            // SAFETY: `data` is the buffer pointer provided by AAudio for this callback.
            unsafe {
                std::slice::from_raw_parts_mut(data as *mut u8, byte_count).fill(0);
            }

            // Deliver audio data to user callback
            let cb_info = OutputCallbackInfo {
                timestamp: OutputStreamTimestamp {
                    callback: now_stream_instant(),
                    playback: output_stream_instant(stream, sample_rate),
                },
            };
            (data_callback)(
                &mut unsafe { Data::from_parts(data as *mut _, n_samples, sample_format) },
                &cb_info,
            );

            // Dynamic buffer tuning for output streams
            // See: https://developer.android.com/ndk/guides/audio/aaudio/aaudio#tuning-buffers
            if tune_dynamically {
                let underrun_count = stream.x_run_count();
                let previous = tuning_for_callback
                    .previous_underrun_count
                    .load(Ordering::Relaxed);

                if underrun_count > previous {
                    // The number of frames per burst can vary dynamically
                    let mut burst_size = stream.frames_per_burst();
                    if burst_size <= 0 {
                        burst_size = 256; // fallback from AAudio documentation
                    } else if burst_size < 16 {
                        burst_size = 16; // floor from Oboe
                    }

                    let new_mixer_bursts = tuning_for_callback
                        .mixer_bursts
                        .load(Ordering::Relaxed)
                        .saturating_add(1);
                    let mut buffer_size = burst_size * new_mixer_bursts;

                    let buffer_capacity = tuning_for_callback.capacity.load(Ordering::Relaxed);
                    if buffer_size > buffer_capacity {
                        buffer_size = buffer_capacity;
                    }

                    if stream.set_buffer_size_in_frames(buffer_size).is_ok() {
                        tuning_for_callback
                            .mixer_bursts
                            .store(new_mixer_bursts, Ordering::Relaxed);
                    }

                    tuning_for_callback
                        .previous_underrun_count
                        .store(underrun_count, Ordering::Relaxed);
                }
            }

            ndk::audio::AudioCallbackResult::Continue
        }))
        .error_callback(Box::new(move |_stream, error| {
            emit_error(&error_callback_for_stream, Error::from(error));
        }))
        .open_stream()?;

    // After stream opens, query and cache the values
    let capacity = stream.buffer_capacity_in_frames();
    tuning.capacity.store(capacity, Ordering::Relaxed);

    let mixer_bursts = match AudioManager::get_mixer_bursts() {
        Ok(bursts) => bursts.max(0),
        Err(_) => {
            let burst_size = stream.frames_per_burst();
            if burst_size > 0 {
                stream.buffer_size_in_frames() / burst_size
            } else {
                0 // defer to dynamic tuning
            }
        }
    };
    tuning.mixer_bursts.store(mixer_bursts, Ordering::Relaxed);

    // SAFETY: Stream implements Send + Sync (see unsafe impl below). Arc<Mutex<AudioStream>>
    // is safe because the Mutex provides exclusive access and AudioStream's thread safety
    // is documented in the AAudio C API.
    #[allow(clippy::arc_with_non_send_sync)]
    Ok(Stream {
        inner: Arc::new(Mutex::new(stream)),
        direction: DeviceDirection::Output,
    })
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        match &self.0 {
            None => Ok(DeviceDescriptionBuilder::new("Default Device".to_string()).build()),
            Some(info) => {
                let device_type: DeviceType = info.device_type.into();
                let name = match device_type {
                    DeviceType::Unknown => info.product_name.clone(),
                    _ => format!("{} ({})", info.product_name, device_type),
                };
                let mut builder = DeviceDescriptionBuilder::new(name)
                    .device_type(device_type)
                    .interface_type(info.device_type.into())
                    .direction(info.direction);

                // Add address if not empty
                if !info.address.is_empty() {
                    builder = builder.address(info.address.clone());
                }

                Ok(builder.build())
            }
        }
    }

    fn id(&self) -> Result<DeviceId, Error> {
        let device_str = match &self.0 {
            None => "-1".to_string(), // Default device
            Some(info) => info.id.to_string(),
        };
        Ok(DeviceId(crate::platform::HostId::AAudio, device_str))
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        if let Some(info) = &self.0 {
            // Output-only devices do not support input
            if matches!(info.direction, DeviceDirection::Output) {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedOperation,
                    "output-only device does not support input",
                ));
            }
            Ok(device_supported_configs(info))
        } else {
            Ok(default_supported_configs())
        }
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        if let Some(info) = &self.0 {
            // Input-only devices do not support output
            if matches!(info.direction, DeviceDirection::Input) {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedOperation,
                    "input-only device does not support output",
                ));
            }
            Ok(device_supported_configs(info))
        } else {
            Ok(default_supported_configs())
        }
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        let mut configs: Vec<_> = self.supported_input_configs()?.collect();
        configs.sort_by(|a, b| b.cmp_default_heuristics(a));
        let range = configs.into_iter().next().ok_or_else(|| {
            Error::with_message(
                ErrorKind::UnsupportedConfig,
                "no supported input configuration",
            )
        })?;
        let config = range
            .try_with_standard_sample_rate()
            .unwrap_or_else(|| range.with_max_sample_rate());
        Ok(config)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        let mut configs: Vec<_> = self.supported_output_configs()?.collect();
        configs.sort_by(|a, b| b.cmp_default_heuristics(a));
        let range = configs.into_iter().next().ok_or_else(|| {
            Error::with_message(
                ErrorKind::UnsupportedConfig,
                "no supported output configuration",
            )
        })?;
        let config = range
            .try_with_standard_sample_rate()
            .unwrap_or_else(|| range.with_max_sample_rate());
        Ok(config)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let format = match sample_format {
            SampleFormat::I16 => ndk::audio::AudioFormat::PCM_I16,
            SampleFormat::F32 => ndk::audio::AudioFormat::PCM_Float,
            sample_format => {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!("{sample_format} format is not supported on Android"),
                ))
            }
        };
        let channel_count = match config.channels {
            1 => 1,
            2 => 2,
            channels => {
                // TODO: more channels available in native AAudio
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!("{channels} channels are not supported yet (only 1 or 2)"),
                ));
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
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let format = match sample_format {
            SampleFormat::I16 => ndk::audio::AudioFormat::PCM_I16,
            SampleFormat::F32 => ndk::audio::AudioFormat::PCM_Float,
            sample_format => {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!("{sample_format} format is not supported on Android"),
                ))
            }
        };
        let channel_count = match config.channels {
            1 => 1,
            2 => 2,
            channels => {
                // TODO: more channels available in native AAudio
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!("{channels} channels are not supported yet (only 1 or 2)"),
                ));
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
    fn play(&self) -> Result<(), Error> {
        let stream = self.inner.lock().map_err(|_| {
            Error::with_message(ErrorKind::StreamInvalidated, "stream lock poisoned")
        })?;

        stream
            .request_start()
            .context("failed to start AAudio stream")?;
        stream
            .wait_for_state_change(
                ndk::audio::AudioStreamState::Starting,
                DEFAULT_TIMEOUT_NANOS,
            )
            .map(|_| ())
            .context("failed to wait for AAudio stream to start")
    }

    fn pause(&self) -> Result<(), Error> {
        match self.direction {
            DeviceDirection::Output => {
                let stream = self.inner.lock().map_err(|_| {
                    Error::with_message(ErrorKind::StreamInvalidated, "stream lock poisoned")
                })?;

                stream
                    .request_pause()
                    .context("failed to pause AAudio stream")?;
                stream
                    .wait_for_state_change(
                        ndk::audio::AudioStreamState::Pausing,
                        DEFAULT_TIMEOUT_NANOS,
                    )
                    .map(|_| ())
                    .context("failed to wait for AAudio stream to pause")
            }
            _ => Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "pause only supported on output streams",
            )),
        }
    }

    fn now(&self) -> StreamInstant {
        now_stream_instant()
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        let stream = self.inner.lock().map_err(|_| {
            Error::with_message(ErrorKind::StreamInvalidated, "stream lock poisoned")
        })?;

        // frames_per_data_callback is only set for BufferSize::Fixed; for Default AAudio
        // schedules callbacks at the burst size, so that is the best available estimate.
        let frames = match stream.frames_per_data_callback() {
            Some(size) if size > 0 => size,
            _ => stream.frames_per_burst(),
        };
        Ok(frames as FrameCount)
    }
}
