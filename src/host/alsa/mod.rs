//! ALSA backend implementation.
//!
//! Default backend on Linux and BSD systems.

extern crate alsa;
#[cfg(feature = "realtime")]
extern crate alsa_sys;
extern crate libc;

use std::{
    cmp,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
    vec::IntoIter as VecIntoIter,
};

use self::alsa::poll::Descriptors;
pub use self::enumerate::Devices;
use crate::{
    host::{
        equilibrium::{fill_equilibrium, DSD_EQUILIBRIUM_BYTE, U8_EQUILIBRIUM_BYTE},
        frames_to_duration,
        latch::Latch,
    },
    iter::{SupportedInputConfigs, SupportedOutputConfigs},
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceDirection,
    DeviceId, Error, ErrorKind, FrameCount, InputCallbackInfo, InputStreamTimestamp,
    OutputCallbackInfo, OutputStreamTimestamp, SampleFormat, SampleRate, StreamConfig,
    StreamInstant, SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    COMMON_SAMPLE_RATES,
};

mod enumerate;

// ALSA Buffer Size Behavior
// =========================
//
// ## ALSA Latency Model
//
// **Hardware vs Software Buffer**: ALSA maintains a software buffer in memory that feeds
// a hardware buffer in the audio device. Audio latency is determined by how much data
// sits in the software buffer before being transferred to hardware.
//
// **Period-Based Transfer**: ALSA transfers data in chunks called "periods". When one
// period worth of data has been consumed by hardware, ALSA triggers a callback to refill
// that period in the software buffer.
//
// ## BufferSize::Fixed Behavior
//
// When `BufferSize::Fixed(x)` is specified, cpal attempts to configure the period size
// to approximately `x` frames to achieve the requested callback size. However, the
// actual callback size may differ from the request:
//
// - ALSA may round the period size to hardware-supported values
// - Different devices have different period size constraints
// - The callback size is not guaranteed to exactly match the request
// - If the requested size cannot be accommodated, ALSA will choose the nearest
//   supported configuration
//
// This mirrors the behavior documented in the cpal API where `BufferSize::Fixed(x)`
// requests but does not guarantee a specific callback size.
//
// ## BufferSize::Default Behavior
//
// When `BufferSize::Default` is specified, cpal does NOT set explicit period size or
// period count constraints, allowing the device/driver to choose sensible defaults.
//
// **Why not set defaults?** Different audio systems have different behaviors:
//
// - **Native ALSA hardware**: Typically chooses reasonable defaults (e.g., 512-2048
//   frame periods with 2-4 periods)
//
// - **PipeWire-ALSA plugin**: Allocates a large ring buffer (~1M frames at 48kHz) but
//   uses small periods (512-1024 frames). Critically, if you request `set_periods(2)`
//   without specifying period size, PipeWire calculates period = buffer/2, resulting
//   in pathologically large periods (~524K frames = 10 seconds). See issues #1029 and
//   #1036.
//
// By not constraining period configuration, PipeWire-ALSA can use its optimized defaults
// (small periods with many-period buffer), while native ALSA hardware uses its own defaults.
//
// **Startup latency**: Regardless of buffer size, cpal uses double-buffering for startup
// (start_threshold = 2 periods), ensuring low latency even with large multi-period ring
// buffers.

const DEFAULT_DEVICE: &str = "default";
const DEFAULT_PERIODS: alsa::pcm::Frames = 2;

// Some ALSA plugins (e.g. alsaequal, certain USB drivers) are not reentrant.
static ALSA_OPEN_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

// TODO: Not yet defined in rust-lang/libc crate
const LIBC_ENOTSUPP: libc::c_int = 524;

/// The default Linux and BSD host type.
#[derive(Debug, Clone)]
pub struct Host {
    inner: Arc<AlsaContext>,
}

impl Host {
    pub fn new() -> Result<Self, Error> {
        let inner = AlsaContext::new().map_err(|e| {
            Error::with_message(ErrorKind::HostUnavailable, format!("ALSA unavailable: {e}"))
        })?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume ALSA is always available on Linux and BSD.
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        self.enumerate_devices()
    }

    fn device_by_id(&self, id: &DeviceId) -> Option<Self::Device> {
        let canonical_id = DeviceId(id.0, canonical_pcm_id(&id.1));
        self.devices()
            .ok()?
            .find(|d| d.id().ok().as_ref() == Some(&canonical_id))
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        Some(Self::Device::default())
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        Some(Self::Device::default())
    }
}

/// Global count of active ALSA context instances.
static ALSA_CONTEXT_COUNT: Mutex<usize> = Mutex::new(0);

/// ALSA backend context shared between `Host`, `Device`, and `Stream` via `Arc`.
#[derive(Debug)]
pub(super) struct AlsaContext;

impl AlsaContext {
    fn new() -> Result<Self, alsa::Error> {
        let mut count = ALSA_CONTEXT_COUNT.lock().unwrap_or_else(|e| e.into_inner());
        if *count == 0 {
            alsa::config::update()?;
        }
        *count += 1;
        Ok(Self)
    }
}

impl Drop for AlsaContext {
    fn drop(&mut self) {
        let mut count = ALSA_CONTEXT_COUNT.lock().unwrap_or_else(|e| e.into_inner());
        *count = count.saturating_sub(1);
        if *count == 0 {
            let _ = alsa::config::update_free_global();
        }
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Self::description(self)
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Self::id(self)
    }

    // Override trait defaults to avoid opening devices during enumeration.
    //
    // ALSA does not guarantee transactional cleanup on failed snd_pcm_open(). Opening plugins like
    // alsaequal that fail with EPERM can leak FDs, poisoning the ALSA backend for the process
    // lifetime (subsequent device opens fail with EBUSY until process exit).
    fn supports_input(&self) -> bool {
        matches!(
            self.direction,
            DeviceDirection::Input | DeviceDirection::Duplex
        )
    }

    fn supports_output(&self) -> bool {
        matches!(
            self.direction,
            DeviceDirection::Output | DeviceDirection::Duplex
        )
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Self::supported_input_configs(self)
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Self::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Self::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        Self::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        conf: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Capture)?;
        let stream = Self::Stream::new_input(
            Arc::new(stream_inner),
            data_callback,
            error_callback,
            timeout,
        );
        stream.signal_ready();
        Ok(stream)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        conf: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Playback)?;
        let stream = Self::Stream::new_output(
            Arc::new(stream_inner),
            data_callback,
            error_callback,
            timeout,
        );
        stream.signal_ready();
        Ok(stream)
    }
}

#[derive(Debug)]
struct TriggerSender(libc::c_int);

#[derive(Debug)]
struct TriggerReceiver(libc::c_int);

impl TriggerSender {
    fn wakeup(&self) {
        let buf = 1u64;
        loop {
            let ret = unsafe { libc::write(self.0, &buf as *const u64 as *const _, 8) };
            if ret == 8 {
                return;
            }
            // write() can be interrupted by a signal before writing any bytes; retry.
            assert_eq!(ret, -1, "wakeup: unexpected return value {ret}");
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                panic!("wakeup: {err}");
            }
        }
    }
}

impl TriggerReceiver {
    fn clear_pipe(&self) {
        let mut out = 0u64;
        loop {
            let ret = unsafe { libc::read(self.0, &mut out as *mut u64 as *mut _, 8) };
            if ret == 8 {
                return;
            }
            // read() can be interrupted by a signal before reading any bytes; retry.
            assert_eq!(ret, -1, "clear_pipe: unexpected return value {ret}");
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::Interrupted {
                panic!("clear_pipe: {err}");
            }
        }
    }
}

fn trigger() -> (TriggerSender, Arc<TriggerReceiver>) {
    let mut fds = [0, 0];
    match unsafe { libc::pipe(fds.as_mut_ptr()) } {
        0 => (TriggerSender(fds[1]), Arc::new(TriggerReceiver(fds[0]))),
        _ => panic!("Could not create pipe"),
    }
}

impl Drop for TriggerSender {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

impl Drop for TriggerReceiver {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Device {
    pcm_id: String,
    desc: Option<String>,
    direction: DeviceDirection,
    _context: Arc<AlsaContext>,
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.pcm_id == other.pcm_id
    }
}

impl Eq for Device {}

impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pcm_id.hash(state);
    }
}

impl Device {
    fn build_stream_inner(
        &self,
        conf: StreamConfig,
        sample_format: SampleFormat,
        stream_type: alsa::Direction,
    ) -> Result<StreamInner, Error> {
        // Validate buffer size if Fixed is specified. This is necessary because
        // `set_period_size_near()` with `ValueOr::Nearest` will accept ANY value and return the
        // "nearest" supported value, which could be wildly different (e.g., requesting 4096 frames
        // might return 512 frames if that's "nearest").
        if let BufferSize::Fixed(requested_size) = conf.buffer_size {
            // Note: We use `default_input_config`/`default_output_config` to get the buffer size
            // range. This queries the CURRENT device (`self.pcm_id`), not the default device. The
            // buffer size range is the same across all format configurations for a given device
            // (see `supported_configs()`).
            let supported_config = match stream_type {
                alsa::Direction::Capture => self.default_input_config(),
                alsa::Direction::Playback => self.default_output_config(),
            };
            if let Ok(config) = supported_config {
                if let SupportedBufferSize::Range { min, max } = config.buffer_size {
                    if !(min..=max).contains(&requested_size) {
                        return Err(Error::with_message(
                            ErrorKind::UnsupportedConfig,
                            format!("buffer size {requested_size} is not in the supported range {min}..={max}"),
                        ));
                    }
                }
            }
        }

        let handle = {
            let _guard = ALSA_OPEN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            alsa::pcm::PCM::new(&self.pcm_id, stream_type, true)?
        };

        let hw_params = set_hw_params_from_format(&handle, conf, sample_format)?;
        let (buffer_size, period_size) = set_sw_params_from_format(&handle, stream_type)?;
        if buffer_size == 0 {
            return Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                format!(
                    "device '{}': initialization resulted in a null buffer",
                    self.pcm_id
                ),
            ));
        }

        handle.prepare()?;

        if handle.count() == 0 {
            return Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                format!("device '{}': poll descriptor count is 0", self.pcm_id),
            ));
        }

        // A zero get_htstamp() at prepare time indicates the device does not support hardware timestamps (e.g. PulseAudio ALSA plugin).
        // Related: https://bugs.freedesktop.org/show_bug.cgi?id=88503
        let creation_ts = handle.status()?.get_htstamp();
        let timestamp_mode = if creation_ts.tv_sec == 0 && creation_ts.tv_nsec == 0 {
            TimestampMode::CreationInstant
        } else if hw_params.supports_audio_ts_type(alsa::pcm::AudioTstampType::LinkSynchronized) {
            TimestampMode::AudioLink
        } else {
            TimestampMode::SystemClock
        };
        drop(hw_params);

        if let alsa::Direction::Capture = stream_type {
            handle.start()?;
        }

        let period_size = period_size as usize;
        let frame_size = sample_format.sample_size() * conf.channels as usize;

        let stream_inner = StreamInner {
            dropping: AtomicBool::new(false),
            handle,
            pcm_id: self.pcm_id.clone(),
            sample_format,
            sample_rate: conf.sample_rate,
            frame_size,
            period_size,
            period_samples: period_size * conf.channels as usize,
            equilibrium: EquilibriumFill::new(sample_format, period_size * frame_size),
            timestamp_mode,
            creation_ts,
            creation_instant: std::time::Instant::now(),
            _context: self._context.clone(),
        };

        Ok(stream_inner)
    }

    fn description(&self) -> Result<DeviceDescription, Error> {
        let name = self
            .desc
            .as_ref()
            .and_then(|desc| desc.lines().next())
            .unwrap_or(&self.pcm_id)
            .to_string();

        let mut builder = DeviceDescriptionBuilder::new(name)
            .driver(self.pcm_id.clone())
            .direction(self.direction);

        if let Some(ref desc) = self.desc {
            let lines = desc
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
            builder = builder.extended(lines);
        }

        Ok(builder.build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId(crate::platform::HostId::Alsa, self.pcm_id.clone()))
    }

    fn supported_configs(
        &self,
        stream_t: alsa::Direction,
    ) -> Result<VecIntoIter<SupportedStreamConfigRange>, Error> {
        let pcm = {
            let _guard = ALSA_OPEN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            alsa::pcm::PCM::new(&self.pcm_id, stream_t, true)?
        };

        let hw_params = alsa::pcm::HwParams::any(&pcm)?;

        // Test both LE and BE formats to detect what the hardware actually supports.
        // LE is listed first as it's the common case for most audio hardware.
        // Hardware reports its supported formats regardless of CPU endianness.
        const FORMATS: [(SampleFormat, alsa::pcm::Format); 23] = [
            (SampleFormat::I8, alsa::pcm::Format::S8),
            (SampleFormat::U8, alsa::pcm::Format::U8),
            (SampleFormat::I16, alsa::pcm::Format::S16LE),
            (SampleFormat::I16, alsa::pcm::Format::S16BE),
            (SampleFormat::U16, alsa::pcm::Format::U16LE),
            (SampleFormat::U16, alsa::pcm::Format::U16BE),
            (SampleFormat::I24, alsa::pcm::Format::S24LE),
            (SampleFormat::I24, alsa::pcm::Format::S24BE),
            (SampleFormat::U24, alsa::pcm::Format::U24LE),
            (SampleFormat::U24, alsa::pcm::Format::U24BE),
            (SampleFormat::I32, alsa::pcm::Format::S32LE),
            (SampleFormat::I32, alsa::pcm::Format::S32BE),
            (SampleFormat::U32, alsa::pcm::Format::U32LE),
            (SampleFormat::U32, alsa::pcm::Format::U32BE),
            (SampleFormat::F32, alsa::pcm::Format::FloatLE),
            (SampleFormat::F32, alsa::pcm::Format::FloatBE),
            (SampleFormat::F64, alsa::pcm::Format::Float64LE),
            (SampleFormat::F64, alsa::pcm::Format::Float64BE),
            (SampleFormat::DsdU8, alsa::pcm::Format::DSDU8),
            (SampleFormat::DsdU16, alsa::pcm::Format::DSDU16LE),
            (SampleFormat::DsdU16, alsa::pcm::Format::DSDU16BE),
            (SampleFormat::DsdU32, alsa::pcm::Format::DSDU32LE),
            (SampleFormat::DsdU32, alsa::pcm::Format::DSDU32BE),
            //SND_PCM_FORMAT_IEC958_SUBFRAME_LE,
            //SND_PCM_FORMAT_IEC958_SUBFRAME_BE,
            //SND_PCM_FORMAT_MU_LAW,
            //SND_PCM_FORMAT_A_LAW,
            //SND_PCM_FORMAT_IMA_ADPCM,
            //SND_PCM_FORMAT_MPEG,
            //SND_PCM_FORMAT_GSM,
            //SND_PCM_FORMAT_SPECIAL,
            //SND_PCM_FORMAT_S24_3LE,
            //SND_PCM_FORMAT_S24_3BE,
            //SND_PCM_FORMAT_U24_3LE,
            //SND_PCM_FORMAT_U24_3BE,
            //SND_PCM_FORMAT_S20_3LE,
            //SND_PCM_FORMAT_S20_3BE,
            //SND_PCM_FORMAT_U20_3LE,
            //SND_PCM_FORMAT_U20_3BE,
            //SND_PCM_FORMAT_S18_3LE,
            //SND_PCM_FORMAT_S18_3BE,
            //SND_PCM_FORMAT_U18_3LE,
            //SND_PCM_FORMAT_U18_3BE,
        ];

        // Collect supported formats, deduplicating since we test both LE and BE variants.
        // If hardware supports both endiannesses (rare), we only report the format once.
        let mut supported_formats = Vec::new();
        for &(sample_format, alsa_format) in FORMATS.iter() {
            if hw_params.test_format(alsa_format).is_ok()
                && !supported_formats.contains(&sample_format)
            {
                supported_formats.push(sample_format);
            }
        }

        let min_rate = hw_params.get_rate_min()?;
        let max_rate = hw_params.get_rate_max()?;

        let sample_rates = if min_rate == max_rate || hw_params.test_rate(min_rate + 1).is_ok() {
            vec![(min_rate, max_rate)]
        } else {
            let mut rates = Vec::new();
            for &sample_rate in COMMON_SAMPLE_RATES.iter() {
                if hw_params.test_rate(sample_rate).is_ok() {
                    rates.push((sample_rate, sample_rate));
                }
            }

            if rates.is_empty() {
                vec![(min_rate, max_rate)]
            } else {
                rates
            }
        };

        let min_channels = hw_params.get_channels_min()?;
        let max_channels = hw_params.get_channels_max()?;

        let max_channels = cmp::min(max_channels, 32); // TODO: limiting to 32 channels or too much stuff is returned
        let supported_channels = (min_channels..=max_channels)
            .filter_map(|num| {
                if hw_params.test_channels(num).is_ok() {
                    Some(num as ChannelCount)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let (min_buffer_size, max_buffer_size) = hw_params_buffer_size_min_max(&hw_params);
        let buffer_size_range = SupportedBufferSize::Range {
            min: min_buffer_size,
            max: max_buffer_size,
        };

        let mut output = Vec::with_capacity(
            supported_formats.len() * supported_channels.len() * sample_rates.len(),
        );
        for &sample_format in supported_formats.iter() {
            for &channels in supported_channels.iter() {
                for &(min_rate, max_rate) in sample_rates.iter() {
                    output.push(SupportedStreamConfigRange {
                        channels,
                        min_sample_rate: min_rate,
                        max_sample_rate: max_rate,
                        buffer_size: buffer_size_range,
                        sample_format,
                    });
                }
            }
        }

        Ok(output.into_iter())
    }

    fn supported_input_configs(&self) -> Result<SupportedInputConfigs, Error> {
        self.supported_configs(alsa::Direction::Capture)
    }

    fn supported_output_configs(&self) -> Result<SupportedOutputConfigs, Error> {
        self.supported_configs(alsa::Direction::Playback)
    }

    // ALSA does not offer default stream formats, so instead we compare all supported formats by
    // the `SupportedStreamConfigRange::cmp_default_heuristics` order and select the greatest.
    fn default_config(&self, stream_t: alsa::Direction) -> Result<SupportedStreamConfig, Error> {
        let mut formats: Vec<_> = {
            match self.supported_configs(stream_t) {
                // EINVAL when querying direction the device does not support (input-only or output-only)
                Err(err) if err.kind() == ErrorKind::InvalidInput => {
                    return Err(Error::with_message(
                        ErrorKind::UnsupportedOperation,
                        format!(
                            "device '{}' does not support the requested direction",
                            self.pcm_id
                        ),
                    ));
                }
                Err(err) => return Err(err),
                Ok(fmts) => fmts.collect(),
            }
        };

        formats.sort_by(|a, b| a.cmp_default_heuristics(b));

        match formats.into_iter().next_back() {
            Some(f) => Ok(f
                .try_with_standard_sample_rate()
                .unwrap_or_else(|| f.with_max_sample_rate())),
            None => Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!("device '{}': no supported configuration", self.pcm_id),
            )),
        }
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config(alsa::Direction::Capture)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        self.default_config(alsa::Direction::Playback)
    }
}

impl Default for Device {
    fn default() -> Self {
        // "default" is a virtual ALSA device that redirects to the configured default. We cannot
        // determine its actual capabilities without opening it, so we return Unknown direction.
        Self {
            pcm_id: DEFAULT_DEVICE.to_owned(),
            desc: Some("Default Audio Device".to_string()),
            direction: DeviceDirection::Unknown,
            _context: Arc::new(
                AlsaContext::new().expect("Failed to initialize ALSA configuration"),
            ),
        }
    }
}

/// Strategy for pre-filling an output buffer with the equilibrium value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum EquilibriumFill {
    /// Equilibrium is represented as a single repeating byte value.
    Byte(u8),
    /// A period-sized buffer pre-filled with the equilibrium value.
    Template(Box<[u8]>),
}

impl EquilibriumFill {
    /// Compute the equilibrium-fill strategy for the given sample format at stream creation.
    fn new(sample_format: SampleFormat, period_bytes: usize) -> Self {
        if sample_format.is_int() || sample_format.is_float() {
            Self::Byte(0)
        } else if sample_format == SampleFormat::U8 {
            Self::Byte(U8_EQUILIBRIUM_BYTE)
        } else if sample_format.is_dsd() {
            Self::Byte(DSD_EQUILIBRIUM_BYTE)
        } else {
            // Multi-byte unsigned integer formats require a fill equal to the midpoint of their
            // range.
            debug_assert!(sample_format.is_uint());
            let mut template = vec![0u8; period_bytes].into_boxed_slice();
            fill_equilibrium(&mut template, sample_format);
            Self::Template(template)
        }
    }

    #[inline]
    fn fill(&self, buffer: &mut [u8]) {
        match self {
            Self::Byte(b) => buffer.fill(*b),
            Self::Template(t) => buffer.copy_from_slice(t),
        }
    }
}

// How callback timestamps are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TimestampMode {
    // Hardware timestamps are unavailable (e.g. PulseAudio ALSA plugin returns zero htstamp).
    // Timestamps are monotonic elapsed time since stream creation, sourced from Instant::now().
    CreationInstant,

    // The kernel records the monotonic clock at each DMA interrupt in htstamp.
    // Subtracting creation_ts (same clock, captured at prepare time) gives elapsed time
    // since stream creation. Uses CLOCK_MONOTONIC_RAW when available, CLOCK_MONOTONIC otherwise.
    SystemClock,

    // The hardware maps the audio sample counter to CLOCK_MONOTONIC_RAW via TSC
    // cross-timestamps (LinkSynchronized), giving a timestamp that tracks the actual audio
    // clock rather than DMA interrupt delivery time. Higher fidelity than SystemClock.
    AudioLink,
}

#[derive(Debug)]
struct StreamInner {
    // Flag used to check when to stop polling, regardless of the state of the stream
    // (e.g. broken due to a disconnected device).
    dropping: AtomicBool,

    // The ALSA handle.
    handle: alsa::pcm::PCM,

    // ALSA PCM identifier used to open this stream.
    pcm_id: String,

    // Format of the samples.
    sample_format: SampleFormat,

    // Sample rate of the stream.
    sample_rate: SampleRate,

    // Cached values for performance in audio callback hot path.
    frame_size: usize,
    period_size: usize,
    period_samples: usize,
    equilibrium: EquilibriumFill,

    // How callback timestamps are produced.
    timestamp_mode: TimestampMode,

    // htstamp value from the status query at prepare() time.
    // Used as the creation-time anchor for SystemClock and AudioLink calculations.
    creation_ts: libc::timespec,

    // Monotonic instant captured at stream creation. Timestamp origin for CreationInstant
    // mode and last-resort fallback if the status query in now() fails.
    creation_instant: std::time::Instant,

    // Keep ALSA context alive to prevent premature ALSA config cleanup.
    _context: Arc<AlsaContext>,
}

// Assume that the ALSA library is built with thread safe option.
unsafe impl Sync for StreamInner {}

#[derive(Debug)]
pub struct Stream {
    /// The high-priority audio processing thread calling callbacks.
    /// Option used for moving out in destructor.
    thread: Option<JoinHandle<()>>,

    /// Handle to the underlying stream for playback controls.
    inner: Arc<StreamInner>,

    /// Used to signal to stop processing.
    trigger: TriggerSender,

    /// Keeps the read end of the self-pipe alive for the lifetime of the Stream, so that
    /// `trigger.wakeup()` never writes to a closed pipe, even if the worker exited early.
    _rx: Arc<TriggerReceiver>,

    /// Latch that prevents the worker thread from firing callbacks until the caller has received
    /// the `Stream` handle.
    latch: Latch,
}

// Compile-time assertion that Stream is Send and Sync
crate::assert_stream_send!(Stream);
crate::assert_stream_sync!(Stream);

impl StreamInner {
    #[inline]
    fn callback_instant(&self, status: &alsa::pcm::Status) -> StreamInstant {
        // For playback the PCM starts in PREPARED state while the output buffer fills;
        // snd_pcm_start() fires automatically at start_threshold, moving it to RUNNING.
        // Therefore, callbacks arrive before RUNNING state. Using creation_ts as the
        // anchor for all modes means timestamps advance monotonically through both the
        // initial buffer fill and any later xrun recovery.
        match self.timestamp_mode {
            TimestampMode::CreationInstant => {
                let d = std::time::Instant::now().duration_since(self.creation_instant);
                StreamInstant::new(d.as_secs(), d.subsec_nanos())
            }
            TimestampMode::SystemClock => {
                // htstamp is the time of the most recent DMA interrupt on the configured
                // monotonic clock. Subtracting creation_ts (same clock, prepare() time)
                // gives elapsed time since stream creation in any PCM state.
                htstamp_elapsed(status, self.creation_ts)
            }
            TimestampMode::AudioLink => {
                // audio_htstamp measures elapsed time since snd_pcm_start() via hardware
                // sample counter and TSC cross-timestamp, so it is only valid in RUNNING state.
                if status.get_state() != alsa::pcm::State::Running {
                    // After xrun recovery, snd_pcm_prepare() does not reset trigger_htstamp
                    // (only snd_pcm_start() does), so it keeps its pre-xrun value while the
                    // hardware counter has not yet restarted.
                    htstamp_elapsed(status, self.creation_ts)
                } else {
                    // When running, add (trigger_ts − creation_ts) to express elapsed time
                    // since stream creation rather than since the last snd_pcm_start().
                    let trigger_ts = status.get_trigger_htstamp();
                    let trigger_offset = timespec_diff_nanos(trigger_ts, self.creation_ts);
                    if trigger_offset < 0 {
                        // trigger_ts predates creation_ts (driver bug); fall back to
                        // htstamp − creation_ts to preserve a monotone result.
                        htstamp_elapsed(status, self.creation_ts)
                    } else {
                        let audio_ts = status.get_audio_htstamp();
                        let nanos = timespec_to_nanos(audio_ts) + trigger_offset;
                        StreamInstant::from_nanos(nanos as u64)
                    }
                }
            }
        }
    }
}

struct StreamWorkerContext {
    descriptors: Box<[libc::pollfd]>,
    transfer_buffer: Box<[u8]>,
    poll_timeout: i32,
}

impl StreamWorkerContext {
    fn new(poll_timeout: &Option<Duration>, stream: &StreamInner, rx: &TriggerReceiver) -> Self {
        let poll_timeout: i32 = if let Some(d) = poll_timeout {
            d.as_millis().try_into().unwrap()
        } else {
            -1 // Don't timeout, wait forever.
        };

        // Pre-allocate a period-sized working buffer. Contents are overwritten each callback.
        let transfer_buffer = vec![0u8; stream.period_size * stream.frame_size].into_boxed_slice();

        // Pre-allocate and initialize descriptors vector: 1 for self-pipe + ALSA descriptors.
        // The descriptor count is constant for the lifetime of stream parameters, and
        // poll() overwrites revents on each call, so we only need to set up fd and events once.
        let num_descriptors = stream.handle.count();
        let total_descriptors = 1 + num_descriptors;
        let mut descriptors = vec![
            libc::pollfd {
                fd: 0,
                events: 0,
                revents: 0
            };
            total_descriptors
        ]
        .into_boxed_slice();

        // Set up self-pipe descriptor at index 0
        descriptors[0] = libc::pollfd {
            fd: rx.0,
            events: libc::POLLIN,
            revents: 0,
        };

        // Set up ALSA descriptors starting at index 1
        let filled = stream
            .handle
            .fill(&mut descriptors[1..])
            .expect("Failed to fill ALSA descriptors");
        debug_assert_eq!(filled, num_descriptors);

        Self {
            descriptors,
            transfer_buffer,
            poll_timeout,
        }
    }
}

fn input_stream_worker(
    rx: Arc<TriggerReceiver>,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(Error) + Send + 'static),
    timeout: Option<Duration>,
) {
    #[cfg(feature = "realtime")]
    if let Err(err) = boost_current_thread_priority(stream) {
        error_callback(err);
    }

    let mut ctxt = StreamWorkerContext::new(&timeout, stream, &rx);
    loop {
        if stream.dropping.load(Ordering::Acquire) {
            return;
        }
        let result = match poll_for_period(&rx, stream, &mut ctxt) {
            Ok(Poll::Pending) => continue,
            Ok(Poll::Ready {
                status,
                delay_frames,
            }) => process_input(
                stream,
                &mut ctxt.transfer_buffer,
                status,
                delay_frames,
                data_callback,
            ),
            Err(err) => Err(err),
        };
        if let Err(err) = result {
            match err.kind() {
                ErrorKind::Xrun => {
                    error_callback(err);
                    if let Err(err) = stream.handle.prepare() {
                        error_callback(err.into());
                    } else if let Err(err) = stream.handle.start() {
                        error_callback(err.into());
                    }
                }
                ErrorKind::DeviceNotAvailable => {
                    error_callback(err);
                    return;
                }
                _ => error_callback(err),
            }
        }
    }
}

fn output_stream_worker(
    rx: Arc<TriggerReceiver>,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(Error) + Send + 'static),
    timeout: Option<Duration>,
) {
    #[cfg(feature = "realtime")]
    if let Err(err) = boost_current_thread_priority(stream) {
        error_callback(err);
    }

    let mut ctxt = StreamWorkerContext::new(&timeout, stream, &rx);

    loop {
        if stream.dropping.load(Ordering::Acquire) {
            return;
        }
        let result = match poll_for_period(&rx, stream, &mut ctxt) {
            Ok(Poll::Pending) => continue,
            Ok(Poll::Ready {
                status,
                delay_frames,
            }) => process_output(
                stream,
                &mut ctxt.transfer_buffer,
                status,
                delay_frames,
                data_callback,
            ),
            Err(err) => Err(err),
        };
        if let Err(err) = result {
            match err.kind() {
                ErrorKind::Xrun => {
                    error_callback(err);
                    if let Err(err) = stream.handle.prepare() {
                        error_callback(err.into());
                    }
                    // No need to call start() for output streams after prepare();
                    // ALSA automatically restarts them when the buffer is refilled
                    // and the stream is triggered again.
                }
                ErrorKind::DeviceNotAvailable => {
                    error_callback(err);
                    return;
                }
                _ => error_callback(err),
            }
        }
    }
}

#[cfg(feature = "realtime")]
fn boost_current_thread_priority(
    stream: &StreamInner,
) -> Result<audio_thread_priority::RtPriorityHandle, Error> {
    use alsa_sys::*;
    // SAFETY: `alsa::pcm::PCM` is `pub struct PCM(*mut snd_pcm_t, Cell<bool>)`. The crate
    // does not expose a public `as_ptr()`, but we can cast and read from it.
    // TODO: replace with `stream.handle.as_ptr()` once alsa-rs exposes it publicly.
    let raw = unsafe {
        (&stream.handle as *const alsa::pcm::PCM)
            .cast::<*mut snd_pcm_t>()
            .read()
    };
    let pcm_type = unsafe { snd_pcm_type(raw) };

    // Only promote to RT for kernel-backed and pure-computation plugins. Others can exhaust
    // RLIMIT_RTTIME when they block or coordinate with non-RT servers and trigger SIGXCPU
    // on an RT thread. IOPLUG and EXTPLUG are excluded: no reliable way to distinguish
    // RT-safe drivers (e.g. pipewire-alsa) from server-backed ones (e.g. pcm_pulse).
    if !matches!(
        pcm_type,
        SND_PCM_TYPE_HW
            | SND_PCM_TYPE_HOOKS
            | SND_PCM_TYPE_NULL
            | SND_PCM_TYPE_COPY
            | SND_PCM_TYPE_LINEAR
            | SND_PCM_TYPE_ALAW
            | SND_PCM_TYPE_MULAW
            | SND_PCM_TYPE_ADPCM
            | SND_PCM_TYPE_RATE
            | SND_PCM_TYPE_ROUTE
            | SND_PCM_TYPE_PLUG
            | SND_PCM_TYPE_LINEAR_FLOAT
            | SND_PCM_TYPE_IEC958
            | SND_PCM_TYPE_SOFTVOL
    ) {
        let type_name = unsafe {
            std::ffi::CStr::from_ptr(snd_pcm_type_name(pcm_type))
                .to_str()
                .unwrap_or("unknown")
        };
        return Err(Error::with_message(
            ErrorKind::RealtimeDenied,
            format!(
                "device '{}' ({type_name}) cannot be promoted to real-time priority",
                stream.pcm_id,
            ),
        ));
    }

    let period_frames = u32::try_from(stream.period_size).unwrap_or(0);
    audio_thread_priority::promote_current_thread_to_real_time(period_frames, stream.sample_rate)
        .map_err(Error::from)
}

/// Attempt hardware resume from a suspend event (`ESTRPIPE`).
fn try_resume(handle: &alsa::PCM) -> Result<Poll, Error> {
    let hw_params = handle.hw_params_current()?;
    if !hw_params.can_resume() {
        return Err(Error::with_message(
            ErrorKind::Xrun, // treat as xrun so the worker calls prepare()
            "hardware suspend/resume not supported",
        ));
    }

    match handle.resume() {
        Ok(()) => {
            if handle
                .info()
                .map(|i| i.get_stream() == alsa::Direction::Capture)
                .unwrap_or(false)
            {
                // A successful `resume()` may leave the device `PREPARED` rather than `RUNNING`.
                // `start()` to ensure the capture actually resumes.
                if let Err(e) = handle.start() {
                    // `EBUSY` is ignored because it means the device is already running.
                    if e.errno() != libc::EBUSY {
                        return Err(e.into());
                    }
                }
            }
            Ok(Poll::Pending)
        }
        // device is still resuming; poll again until it is ready.
        Err(e) if e.errno() == libc::EAGAIN => Ok(Poll::Pending),
        // hardware does not support soft resume; treat as xrun so the worker calls prepare()
        Err(e) if e.errno() == libc::ENOSYS => {
            Err(Error::with_message(ErrorKind::Xrun, e.to_string()))
        }
        Err(e) => Err(e.into()),
    }
}

enum Poll {
    Pending,
    Ready {
        status: alsa::pcm::Status,
        delay_frames: usize,
    },
}

// This block is shared between both input and output stream worker functions.
fn poll_for_period(
    rx: &TriggerReceiver,
    stream: &StreamInner,
    ctxt: &mut StreamWorkerContext,
) -> Result<Poll, Error> {
    let StreamWorkerContext {
        ref mut descriptors,
        ref poll_timeout,
        ..
    } = *ctxt;

    let res = alsa::poll::poll(descriptors, *poll_timeout)?;
    if res == 0 {
        // poll() returned 0: either a timeout or a spurious wakeup. Nothing to do.
        return Ok(Poll::Pending);
    }

    if descriptors[0].revents != 0 {
        // Self-pipe fired: the stream is being dropped. Clear the pipe and let the
        // worker loop detect the dropping flag on the next iteration.
        rx.clear_pipe();
        return Ok(Poll::Pending);
    }

    let revents = stream.handle.revents(&descriptors[1..])?;
    // No events: spurious wakeup, poll again.
    if revents.is_empty() {
        return Ok(Poll::Pending);
    }
    // POLLHUP/POLLNVAL: the device has been disconnected.
    if revents.intersects(alsa::poll::Flags::HUP | alsa::poll::Flags::NVAL) {
        return Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            format!("device '{}' disconnected", stream.pcm_id),
        ));
    }
    // POLLERR signals an xrun or suspend; avail_delay() below returns EPIPE/ESTRPIPE accordingly.
    // POLLIN/POLLOUT: data is ready, fall through to process it.
    let (avail_frames, delay_frames) = match stream.handle.avail_delay() {
        // Xrun: recover via prepare() (+ start() for capture, handled by the worker).
        Err(err) if err.errno() == libc::EPIPE => {
            return Err(Error::with_message(ErrorKind::Xrun, err.to_string()))
        }
        // Suspend: try hardware resume first; fall back to prepare() if unsupported.
        Err(err) if err.errno() == libc::ESTRPIPE => return try_resume(&stream.handle),
        res => res,
    }?;
    // ALSA can have spurious wakeups where poll returns but avail < avail_min.
    // This is documented to occur with dmix (timer-driven) and other plugins.
    // Verify we have room for at least one full period before processing.
    // See: https://bugzilla.kernel.org/show_bug.cgi?id=202499
    //
    // Compare in Frames (i64) so that a negative avail_frames from a buggy driver
    // naturally fails the guard rather than wrapping to a huge usize that passes it.
    if avail_frames < stream.period_size as alsa::pcm::Frames {
        return Ok(Poll::Pending);
    }

    let audio_ts_type = match stream.timestamp_mode {
        TimestampMode::AudioLink => alsa::pcm::AudioTstampType::LinkSynchronized,
        TimestampMode::SystemClock | TimestampMode::CreationInstant => {
            alsa::pcm::AudioTstampType::Compat
        }
    };
    // From the guard above we know that this poll is not a spurious wakeup,
    // so we also know we can query the device in a stable state.
    let status = alsa::pcm::StatusBuilder::new()
        .audio_htstamp_config(audio_ts_type, false)
        .build(&stream.handle)?;

    Ok(Poll::Ready {
        status,
        delay_frames: delay_frames.max(0) as usize,
    })
}

// Read input data from ALSA and deliver it to the user.
fn process_input(
    stream: &StreamInner,
    buffer: &mut [u8],
    status: alsa::pcm::Status,
    delay_frames: usize,
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
) -> Result<(), Error> {
    let mut frames_read = 0;
    while frames_read < stream.period_size {
        match stream
            .handle
            .io_bytes()
            .readi(&mut buffer[frames_read * stream.frame_size..])
        {
            Ok(n) => frames_read += n,
            // EAGAIN = no frames available: skip this cycle if no progress was made,
            // otherwise treat as an underrun (partial period cannot be delivered safely).
            Err(err) if err.errno() == libc::EAGAIN => {
                if frames_read == 0 {
                    return Ok(());
                } else {
                    return Err(Error::with_message(ErrorKind::Xrun, err.to_string()));
                }
            }
            // EPIPE = xrun: full underrun recovery (prepare + start) required.
            Err(err) if err.errno() == libc::EPIPE => {
                return Err(Error::with_message(ErrorKind::Xrun, err.to_string()))
            }
            // ESTRPIPE = hardware suspend: try soft resume first, falling back to underrun
            // recovery if the hardware doesn't support it.
            Err(err) if err.errno() == libc::ESTRPIPE => {
                return try_resume(&stream.handle).map(|_| ());
            }
            Err(err) => return Err(err.into()),
        }
    }
    let data = buffer.as_mut_ptr() as *mut ();
    let data = unsafe { Data::from_parts(data, stream.period_samples, stream.sample_format) };
    let callback_instant = stream.callback_instant(&status);
    let delay_duration = frames_to_duration(delay_frames as FrameCount, stream.sample_rate);
    let capture = callback_instant
        .checked_sub(delay_duration)
        .unwrap_or(StreamInstant::ZERO);
    let timestamp = InputStreamTimestamp {
        callback: callback_instant,
        capture,
    };
    let info = InputCallbackInfo { timestamp };
    data_callback(&data, &info);

    Ok(())
}

// Request data from the user's function and write it via ALSA.
fn process_output(
    stream: &StreamInner,
    buffer: &mut [u8],
    status: alsa::pcm::Status,
    delay_frames: usize,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
) -> Result<(), Error> {
    // Pre-fill buffer with equilibrium; user callback overwrites what it wants.
    stream.equilibrium.fill(buffer);

    let data = buffer.as_mut_ptr() as *mut ();
    let mut data = unsafe { Data::from_parts(data, stream.period_samples, stream.sample_format) };
    let callback_instant = stream.callback_instant(&status);
    let delay_duration = frames_to_duration(delay_frames as FrameCount, stream.sample_rate);
    let playback = callback_instant + delay_duration;
    let timestamp = OutputStreamTimestamp {
        callback: callback_instant,
        playback,
    };
    let info = OutputCallbackInfo { timestamp };
    data_callback(&mut data, &info);

    let mut frames_written = 0;
    while frames_written < stream.period_size {
        match stream
            .handle
            .io_bytes()
            .writei(&buffer[frames_written * stream.frame_size..])
        {
            Ok(n) => frames_written += n,
            // EAGAIN = device cannot currently accept more frames: skip this cycle if no
            // progress was made, otherwise treat as an underrun (partial period cannot be
            // completed safely).
            Err(err) if err.errno() == libc::EAGAIN => {
                if frames_written == 0 {
                    return Ok(());
                } else {
                    return Err(Error::with_message(ErrorKind::Xrun, err.to_string()));
                }
            }
            // EPIPE = xrun: full underrun recovery (prepare) required.
            Err(err) if err.errno() == libc::EPIPE => {
                return Err(Error::with_message(ErrorKind::Xrun, err.to_string()))
            }
            // ESTRPIPE = hardware suspend: try soft resume first, falling back to underrun
            // recovery if the hardware doesn't support it.
            Err(err) if err.errno() == libc::ESTRPIPE => {
                return try_resume(&stream.handle).map(|_| ());
            }
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

// Adapted from `timestamp2ns` here:
// https://fossies.org/linux/alsa-lib/test/audio_time.c
#[inline]
#[allow(clippy::unnecessary_cast)]
fn timespec_to_nanos(ts: libc::timespec) -> i64 {
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

// Adapted from `timediff` here:
// https://fossies.org/linux/alsa-lib/test/audio_time.c
#[inline]
fn timespec_diff_nanos(a: libc::timespec, b: libc::timespec) -> i64 {
    timespec_to_nanos(a) - timespec_to_nanos(b)
}

// StreamInstant representing how long htstamp is ahead of origin, clamped to zero.
// Used as the creation-relative timestamp source for SystemClock and AudioLink fallback paths.
#[inline]
fn htstamp_elapsed(status: &alsa::pcm::Status, origin: libc::timespec) -> StreamInstant {
    let nanos = timespec_diff_nanos(status.get_htstamp(), origin);
    StreamInstant::from_nanos(nanos.max(0) as u64)
}

impl Stream {
    /// Releases the latch so the worker thread can begin processing audio callbacks.
    pub(crate) fn signal_ready(&self) {
        self.latch.release();
    }

    fn new_input<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
        timeout: Option<Duration>,
    ) -> Stream
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (tx, rx) = trigger();
        let rx_thread = rx.clone();
        let stream = inner.clone();

        // The latch is released just before the `Stream` is returned so the worker cannot fire any
        // callbacks before the caller has the handle.
        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let thread = thread::Builder::new()
            .name("cpal_alsa_in".to_owned())
            .spawn(move || {
                waiter.wait();
                input_stream_worker(
                    rx_thread,
                    &stream,
                    &mut data_callback,
                    &mut error_callback,
                    timeout,
                );
            })
            .unwrap();
        latch.add_thread(thread.thread().clone());

        Self {
            thread: Some(thread),
            inner,
            trigger: tx,
            _rx: rx,
            latch,
        }
    }

    fn new_output<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
        timeout: Option<Duration>,
    ) -> Stream
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (tx, rx) = trigger();
        let rx_thread = rx.clone();
        let stream = inner.clone();

        // The latch is released just before the `Stream` is returned so the worker cannot fire any
        // callbacks before the caller has the handle.
        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let thread = thread::Builder::new()
            .name("cpal_alsa_out".to_owned())
            .spawn(move || {
                waiter.wait();
                output_stream_worker(
                    rx_thread,
                    &stream,
                    &mut data_callback,
                    &mut error_callback,
                    timeout,
                );
            })
            .unwrap();
        latch.add_thread(thread.thread().clone());

        Self {
            thread: Some(thread),
            inner,
            trigger: tx,
            _rx: rx,
            latch,
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Unblock the worker in case the stream is dropped before signal_ready() was
        // called. Idempotent: no effect if the worker is already running.
        self.signal_ready();
        self.inner.dropping.store(true, Ordering::Release);
        self.trigger.wakeup();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        if self.inner.handle.state() == alsa::pcm::State::Paused {
            self.inner.handle.pause(false)?;
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        let hw_params = self.inner.handle.hw_params_current()?;
        if !hw_params.can_pause() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                format!("device '{}' does not support pausing", self.inner.pcm_id),
            ));
        }
        if self.inner.handle.state() != alsa::pcm::State::Paused {
            self.inner.handle.pause(true)?;
        }
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        if self.inner.timestamp_mode != TimestampMode::CreationInstant {
            let audio_ts_type = match self.inner.timestamp_mode {
                TimestampMode::AudioLink => alsa::pcm::AudioTstampType::LinkSynchronized,
                _ => alsa::pcm::AudioTstampType::Compat,
            };
            if let Ok(status) = alsa::pcm::StatusBuilder::new()
                .audio_htstamp_config(audio_ts_type, false)
                .build(&self.inner.handle)
            {
                return self.inner.callback_instant(&status);
            }
        }

        let d = std::time::Instant::now().duration_since(self.inner.creation_instant);
        StreamInstant::new(d.as_secs(), d.subsec_nanos())
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.inner.period_size as FrameCount)
    }
}

// Convert ALSA frames to FrameCount, clamping to valid range.
// ALSA Frames are i64 (64-bit) or i32 (32-bit).
fn clamp_frame_count(buffer_size: alsa::pcm::Frames) -> FrameCount {
    buffer_size.max(1).try_into().unwrap_or(FrameCount::MAX)
}

fn hw_params_buffer_size_min_max(hw_params: &alsa::pcm::HwParams) -> (FrameCount, FrameCount) {
    let min_buf = hw_params
        .get_buffer_size_min()
        .map(clamp_frame_count)
        .unwrap_or(1);
    let max_buf = hw_params
        .get_buffer_size_max()
        .map(clamp_frame_count)
        .unwrap_or(FrameCount::MAX);
    (min_buf, max_buf)
}

fn init_hw_params<'a>(
    pcm_handle: &'a alsa::pcm::PCM,
    config: StreamConfig,
    sample_format: SampleFormat,
) -> Result<alsa::pcm::HwParams<'a>, Error> {
    let hw_params = alsa::pcm::HwParams::any(pcm_handle)?;
    hw_params.set_access(alsa::pcm::Access::RWInterleaved)?;

    // Determine which endianness the hardware actually supports for this format.
    // We prefer native endian (no conversion needed) but fall back to the opposite
    // endian if that's all the hardware supports (e.g., LE USB DAC on BE system).
    let alsa_format = sample_format_to_alsa_format(&hw_params, sample_format)?;
    hw_params.set_format(alsa_format)?;

    hw_params.set_rate(config.sample_rate, alsa::ValueOr::Nearest)?;
    hw_params.set_channels(config.channels as u32)?;
    Ok(hw_params)
}

/// Convert SampleFormat to the appropriate alsa::pcm::Format based on what the hardware supports.
/// Prefers native endian, falls back to non-native if that's all the hardware supports.
fn sample_format_to_alsa_format(
    hw_params: &alsa::pcm::HwParams,
    sample_format: SampleFormat,
) -> Result<alsa::pcm::Format, Error> {
    use alsa::pcm::Format;

    // For each sample format, define (native_endian_format, opposite_endian_format) pairs
    let (native, opposite) = match sample_format {
        SampleFormat::I8 => return Ok(Format::S8), // No endianness
        SampleFormat::U8 => return Ok(Format::U8), // No endianness
        #[cfg(target_endian = "little")]
        SampleFormat::I16 => (Format::S16LE, Format::S16BE),
        #[cfg(target_endian = "big")]
        SampleFormat::I16 => (Format::S16BE, Format::S16LE),
        #[cfg(target_endian = "little")]
        SampleFormat::U16 => (Format::U16LE, Format::U16BE),
        #[cfg(target_endian = "big")]
        SampleFormat::U16 => (Format::U16BE, Format::U16LE),
        #[cfg(target_endian = "little")]
        SampleFormat::I24 => (Format::S24LE, Format::S24BE),
        #[cfg(target_endian = "big")]
        SampleFormat::I24 => (Format::S24BE, Format::S24LE),
        #[cfg(target_endian = "little")]
        SampleFormat::U24 => (Format::U24LE, Format::U24BE),
        #[cfg(target_endian = "big")]
        SampleFormat::U24 => (Format::U24BE, Format::U24LE),
        #[cfg(target_endian = "little")]
        SampleFormat::I32 => (Format::S32LE, Format::S32BE),
        #[cfg(target_endian = "big")]
        SampleFormat::I32 => (Format::S32BE, Format::S32LE),
        #[cfg(target_endian = "little")]
        SampleFormat::U32 => (Format::U32LE, Format::U32BE),
        #[cfg(target_endian = "big")]
        SampleFormat::U32 => (Format::U32BE, Format::U32LE),
        #[cfg(target_endian = "little")]
        SampleFormat::F32 => (Format::FloatLE, Format::FloatBE),
        #[cfg(target_endian = "big")]
        SampleFormat::F32 => (Format::FloatBE, Format::FloatLE),
        #[cfg(target_endian = "little")]
        SampleFormat::F64 => (Format::Float64LE, Format::Float64BE),
        #[cfg(target_endian = "big")]
        SampleFormat::F64 => (Format::Float64BE, Format::Float64LE),
        SampleFormat::DsdU8 => return Ok(Format::DSDU8),
        #[cfg(target_endian = "little")]
        SampleFormat::DsdU16 => (Format::DSDU16LE, Format::DSDU16BE),
        #[cfg(target_endian = "big")]
        SampleFormat::DsdU16 => (Format::DSDU16BE, Format::DSDU16LE),
        #[cfg(target_endian = "little")]
        SampleFormat::DsdU32 => (Format::DSDU32LE, Format::DSDU32BE),
        #[cfg(target_endian = "big")]
        SampleFormat::DsdU32 => (Format::DSDU32BE, Format::DSDU32LE),
        _ => {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!("sample format '{sample_format}' is not supported"),
            ))
        }
    };

    // Try native endian first (optimal - no conversion needed)
    if hw_params.test_format(native).is_ok() {
        return Ok(native);
    }

    // Fall back to opposite endian if hardware only supports that
    if hw_params.test_format(opposite).is_ok() {
        return Ok(opposite);
    }

    Err(Error::with_message(
        ErrorKind::UnsupportedConfig,
        format!("sample format '{sample_format}' is not supported by hardware in any endianness"),
    ))
}

fn set_hw_params_from_format(
    pcm_handle: &alsa::pcm::PCM,
    config: StreamConfig,
    sample_format: SampleFormat,
) -> Result<alsa::pcm::HwParams<'_>, Error> {
    let hw_params = init_hw_params(pcm_handle, config, sample_format)?;

    // When BufferSize::Fixed(x) is specified, we configure double-buffering with
    // buffer_size = 2x and period_size = x. This provides consistent low-latency
    // behavior across different ALSA implementations and hardware.
    if let BufferSize::Fixed(buffer_frames) = config.buffer_size {
        hw_params.set_buffer_size_near(DEFAULT_PERIODS * buffer_frames as alsa::pcm::Frames)?;
        hw_params
            .set_period_size_near(buffer_frames as alsa::pcm::Frames, alsa::ValueOr::Nearest)?;
    }

    // Apply hardware parameters
    pcm_handle.hw_params(&hw_params)?;

    // For BufferSize::Default, constrain to device's configured period with 2-period buffering.
    // PipeWire-ALSA picks a good period size but pairs it with many periods (huge buffer).
    // We need to re-initialize hw_params and set BOTH period and buffer to constrain properly.
    if config.buffer_size == BufferSize::Default {
        if let Ok(period_size) = hw_params.get_period_size().map(|s| s as alsa::pcm::Frames) {
            // Re-initialize hw_params to clear previous constraints
            let hw_params = init_hw_params(pcm_handle, config, sample_format)?;

            // Set both period (to device's chosen value) and buffer (to 2 periods)
            hw_params.set_period_size_near(period_size, alsa::ValueOr::Nearest)?;
            hw_params.set_buffer_size_near(DEFAULT_PERIODS * period_size)?;

            // Re-apply with new constraints
            pcm_handle.hw_params(&hw_params)?;
        }
    }

    pcm_handle.hw_params_current().map_err(Into::into)
}

fn set_sw_params_from_format(
    pcm_handle: &alsa::pcm::PCM,
    stream_type: alsa::Direction,
) -> Result<(alsa::pcm::Frames, alsa::pcm::Frames), Error> {
    let sw_params = pcm_handle.sw_params_current()?;
    let (buffer_size, period_size) = pcm_handle
        .get_params()
        .map(|(b, p)| (b as alsa::pcm::Frames, p as alsa::pcm::Frames))?;

    let start_threshold = match stream_type {
        alsa::Direction::Playback => {
            // Start playback when 2 periods are filled. This ensures consistent low-latency
            // startup regardless of total buffer size (whether 2 or more periods).
            DEFAULT_PERIODS * period_size
        }
        alsa::Direction::Capture => 1,
    };
    sw_params.set_start_threshold(start_threshold)?;
    sw_params.set_avail_min(period_size)?;

    sw_params.set_tstamp_mode(true)?;
    sw_params.set_tstamp_type(alsa::pcm::TstampType::MonotonicRaw)?;

    // tstamp_type param cannot be changed after the device is opened.
    // The default tstamp_type value on most Linux systems is "monotonic",
    // let's try to use it if setting the tstamp_type fails.
    if pcm_handle.sw_params(&sw_params).is_err() {
        sw_params.set_tstamp_type(alsa::pcm::TstampType::Monotonic)?;
        pcm_handle.sw_params(&sw_params)?;
    }

    Ok((buffer_size, period_size))
}

fn canonical_pcm_id(pcm_id: &str) -> String {
    if let Some((prefix, rest)) = pcm_id.split_once(':') {
        let (card_str, device_str) = match rest.split_once(',') {
            Some((c, d)) => (c.trim(), d.trim()),
            None => (rest.trim(), "0"),
        };
        if !card_str.contains('=') {
            if let Ok(device) = device_str.parse::<u32>() {
                return format!("{prefix}:CARD={card_str},DEV={device}");
            }
        }
    }
    pcm_id.to_owned()
}

impl From<alsa::Error> for Error {
    fn from(err: alsa::Error) -> Self {
        match err.errno() {
            libc::ENODEV | libc::ENOENT | LIBC_ENOTSUPP => {
                Error::with_message(ErrorKind::DeviceNotAvailable, err.to_string())
            }
            libc::EPERM | libc::EACCES => {
                Error::with_message(ErrorKind::PermissionDenied, err.to_string())
            }
            libc::EBUSY | libc::EAGAIN => {
                Error::with_message(ErrorKind::DeviceBusy, err.to_string())
            }
            libc::EINVAL => Error::with_message(ErrorKind::InvalidInput, err.to_string()),
            libc::EPIPE => Error::with_message(ErrorKind::Xrun, err.to_string()),
            libc::ENOSYS => Error::with_message(ErrorKind::UnsupportedOperation, err.to_string()),
            _ => Error::with_message(ErrorKind::BackendError, err.to_string()),
        }
    }
}
