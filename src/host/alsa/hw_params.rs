use super::{DEFAULT_PERIODS, alsa};
use crate::{
    BufferSize, ChannelCount, Error, ErrorKind, FrameCount, SampleFormat, StreamConfig,
    SupportedBufferSize,
};

pub(super) fn supported_period_size_range(
    hw_params: &alsa::pcm::HwParams<'_>,
    alsa_format: alsa::pcm::Format,
    channels: ChannelCount,
) -> SupportedBufferSize {
    let p = hw_params.clone();
    if p.set_access(alsa::pcm::Access::RWInterleaved).is_err()
        || p.set_channels(channels as u32).is_err()
        || p.set_format(alsa_format).is_err()
    {
        return SupportedBufferSize::Unknown;
    }
    let Some((min, max)) = hw_params_period_size_min_max(&p) else {
        return SupportedBufferSize::Unknown;
    };
    let min_frames = min.max(1);
    // cpal double-buffers (ring = DEFAULT_PERIODS * period), so the achievable
    // period maximum is also bounded by max_buffer / DEFAULT_PERIODS.
    let effective_max = match p.get_buffer_size_max() {
        Ok(max_buf) if max_buf > 0 => max.min(max_buf / DEFAULT_PERIODS),
        _ => max,
    };
    if effective_max >= min_frames {
        let Ok(min) = min_frames.try_into() else {
            return SupportedBufferSize::Unknown;
        };
        SupportedBufferSize::Range {
            min,
            max: effective_max.try_into().unwrap_or(FrameCount::MAX),
        }
    } else {
        SupportedBufferSize::Unknown
    }
}

pub(super) fn set_hw_params_from_format(
    pcm_handle: &alsa::pcm::PCM,
    config: StreamConfig,
    sample_format: SampleFormat,
) -> Result<alsa::pcm::HwParams<'_>, Error> {
    let hw_params = init_hw_params(pcm_handle, config, sample_format)?;

    // When BufferSize::Fixed(x) is specified, we configure double-buffering with
    // buffer_size = 2x and period_size = x. This provides consistent low-latency
    // behavior across different ALSA implementations and hardware.
    if let BufferSize::Fixed(period_size) = config.buffer_size {
        let period_size = period_size as alsa::pcm::Frames;

        // Validate the requested size against the device's supported ranges using the same PCM
        // handle we'll use for streaming. This avoids a second PCM open (which can disturb
        // hardware clock state on some drivers) while still catching wildly out-of-range
        // requests before set_period_size_near silently rounds them.
        if let Some((min_period, max_period)) = hw_params_period_size_min_max(&hw_params) {
            if !(min_period..=max_period).contains(&period_size) {
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!(
                        "Buffer size {period_size} is not in the supported range {min_period}..={max_period}"
                    ),
                ));
            }
        }

        let buffer_size = DEFAULT_PERIODS * period_size;
        if let Ok(max_buffer) = hw_params.get_buffer_size_max() {
            if max_buffer > 0 && buffer_size > max_buffer {
                let effective_max = max_buffer / DEFAULT_PERIODS;
                return Err(Error::with_message(
                    ErrorKind::UnsupportedConfig,
                    format!(
                        "Buffer size {period_size} exceeds the maximum supported value of {effective_max}"
                    ),
                ));
            }
        }

        hw_params.set_buffer_size_near(buffer_size)?;
        hw_params.set_period_size_near(period_size, alsa::ValueOr::Nearest)?;
    }

    // Apply hardware parameters
    pcm_handle.hw_params(&hw_params)?;

    // For BufferSize::Default, constrain to device's configured period with 2-period buffering.
    // PipeWire-ALSA picks a good period size but pairs it with many periods (huge buffer).
    // We need to re-initialize hw_params and set BOTH period and buffer to constrain properly.
    if config.buffer_size == BufferSize::Default {
        if let Ok(period_size) = hw_params.get_period_size() {
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

// What triggers ALSA's automatic Prepared -> Running transition.
pub(super) enum StartThreshold {
    // Capture: any read request satisfies this trivially - effectively immediate.
    Immediate,
    // Playback: starts once this many periods are queued, regardless of total buffer depth.
    Periods(usize),
    // Never automatically; the caller starts the PCM explicitly.
    Disabled,
}

pub(super) fn set_sw_params_from_format(
    pcm_handle: &alsa::pcm::PCM,
    start_threshold: StartThreshold,
) -> Result<(alsa::pcm::Frames, alsa::pcm::Frames), Error> {
    let sw_params = pcm_handle.sw_params_current()?;
    let (buffer_size, period_size) = pcm_handle
        .get_params()
        .map(|(b, p)| (b as alsa::pcm::Frames, p as alsa::pcm::Frames))?;

    let threshold = match start_threshold {
        StartThreshold::Immediate => 1,
        StartThreshold::Periods(periods) => periods as alsa::pcm::Frames * period_size,
        // boundary is unreachable, so auto-start never fires.
        StartThreshold::Disabled => sw_params.get_boundary()?,
    };
    sw_params.set_start_threshold(threshold)?;
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

fn hw_params_period_size_min_max(
    hw_params: &alsa::pcm::HwParams,
) -> Option<(alsa::pcm::Frames, alsa::pcm::Frames)> {
    let min = hw_params.get_period_size_min().ok()?;
    let max = hw_params.get_period_size_max().ok()?;
    // min=0 means no hardware lower bound (PipeWire reports this on unconstrained params);
    // it is handled in the caller by clamping to 1. max <= 0 is degenerate (or ULONG_MAX
    // wrapping negative), so we return None in that case rather than a misleading range.
    (max > 0 && max >= min).then_some((min, max))
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
                format!("Sample format {sample_format} is not supported"),
            ));
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
        format!("Sample format {sample_format} is not supported in any byte order"),
    ))
}
