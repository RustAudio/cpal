use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
    vec::IntoIter as VecIntoIter,
};

use super::{
    AlsaContext, DEFAULT_PERIODS, Stream, alsa,
    alsa::poll::Descriptors,
    hw_params::{
        StartThreshold, set_hw_params_from_format, set_sw_params_from_format,
        supported_period_size_range,
    },
    open_pcm,
    stream::{
        DuplexCaptureState, DuplexPlaybackState, DuplexStreamInner, EquilibriumFill, StreamInner,
        WorkerControl, creation_timestamp,
    },
};
use crate::{
    COMMON_SAMPLE_RATES, CallbackInfo, ChannelCount, Data, DeviceDescription,
    DeviceDescriptionBuilder, DeviceDirection, DeviceId, DuplexCallbackInfo, DuplexStreamConfig,
    Error, ErrorKind, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange,
    iter::{SupportedInputConfigs, SupportedOutputConfigs},
    traits::DeviceTrait,
};

#[derive(Clone, Debug)]
pub struct Device {
    pub(super) pcm_id: String,
    pub(super) desc: Option<String>,
    pub(super) direction: DeviceDirection,
    pub(super) _context: Arc<AlsaContext>,
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

    fn supports_duplex(&self) -> bool {
        self.direction == DeviceDirection::Duplex
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
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        // Keep `capture` monotonic: avail_delay() varies between cycles, and a capture overrun
        // can make it jump up enough to pull `capture` backward.
        let data_callback = crate::host::monotonic_input_callback(data_callback);
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Capture)?;
        let stream = Self::Stream::new_input(
            Arc::new(stream_inner),
            data_callback,
            error_callback,
            timeout,
        );
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
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        // Keep `playback` monotonic: avail_delay() varies between cycles, and a playback
        // underrun can drain the buffer enough to pull `playback` backward.
        let data_callback = crate::host::monotonic_output_callback(data_callback);
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Playback)?;
        let stream = Self::Stream::new_output(
            Arc::new(stream_inner),
            data_callback,
            error_callback,
            timeout,
        );
        Ok(stream)
    }

    fn build_duplex_stream_raw<D, E>(
        &self,
        config: DuplexStreamConfig,
        input_sample_format: SampleFormat,
        output_sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let stream_inner =
            self.build_duplex_stream_inner(config, input_sample_format, output_sample_format)?;
        let stream = Self::Stream::new_duplex(
            Arc::new(stream_inner),
            data_callback,
            error_callback,
            timeout,
        );
        Ok(stream)
    }
}

impl Device {
    fn build_stream_inner(
        &self,
        conf: StreamConfig,
        sample_format: SampleFormat,
        stream_type: alsa::Direction,
    ) -> Result<StreamInner, Error> {
        crate::validate_stream_config(&conf)?;

        let handle = open_pcm(&self.pcm_id, stream_type)?;

        let hw_params = set_hw_params_from_format(&handle, conf, sample_format)?;
        let start_threshold = match stream_type {
            alsa::Direction::Playback => StartThreshold::Periods(DEFAULT_PERIODS as usize),
            alsa::Direction::Capture => StartThreshold::Immediate,
        };
        let (buffer_size, period_size) = set_sw_params_from_format(&handle, start_threshold)?;
        if buffer_size == 0 || period_size == 0 {
            return Err(ErrorKind::DeviceNotAvailable.into());
        }

        handle.prepare()?;

        if handle.count() == 0 {
            return Err(ErrorKind::DeviceNotAvailable.into());
        }

        let (creation_ts, timestamp_mode) = creation_timestamp(&handle, hw_params)?;

        let period_size = period_size as usize;
        let frame_size = frame_size(sample_format, conf.channels);

        let stream_inner = StreamInner {
            control: WorkerControl::default(),
            direction: stream_type.into(),
            handle,
            sample_format,
            sample_rate: conf.sample_rate,
            frame_size,
            period_size,
            period_samples: period_size * conf.channels as usize,
            equilibrium: (stream_type == alsa::Direction::Playback)
                .then(|| EquilibriumFill::new(sample_format, period_size * frame_size)),
            timestamp_mode,
            creation_ts,
            creation_instant: std::time::Instant::now(),
            pending_xrun: AtomicBool::new(false),
            _context: self._context.clone(),
        };

        Ok(stream_inner)
    }

    // Opens capture and playback from the same pcm_id with matching period/rate and returns the
    // paired inner state used to drive both from one worker thread. Linking happens later, in
    // begin_duplex_playback().
    fn build_duplex_stream_inner(
        &self,
        config: DuplexStreamConfig,
        input_sample_format: SampleFormat,
        output_sample_format: SampleFormat,
    ) -> Result<DuplexStreamInner, Error> {
        let capture_config = StreamConfig {
            channels: config.input_channels,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };
        let playback_config = StreamConfig {
            channels: config.output_channels,
            sample_rate: config.sample_rate,
            buffer_size: config.buffer_size,
        };
        crate::validate_stream_config(&capture_config)?;
        crate::validate_stream_config(&playback_config)?;

        let capture_handle = open_pcm(&self.pcm_id, alsa::Direction::Capture)?;
        let playback_handle = open_pcm(&self.pcm_id, alsa::Direction::Playback)?;

        let capture_hw_params =
            set_hw_params_from_format(&capture_handle, capture_config, input_sample_format)?;
        let playback_hw_params =
            set_hw_params_from_format(&playback_handle, playback_config, output_sample_format)?;

        let (capture_buffer_size, capture_period_size) =
            set_sw_params_from_format(&capture_handle, StartThreshold::Disabled)?;
        let (playback_buffer_size, playback_period_size) =
            set_sw_params_from_format(&playback_handle, StartThreshold::Disabled)?;
        if capture_buffer_size == 0
            || capture_period_size == 0
            || playback_buffer_size == 0
            || playback_period_size == 0
        {
            return Err(ErrorKind::DeviceNotAvailable.into());
        }
        // Duplex drives both directions from one worker cycle; period sizes must match.
        if capture_period_size != playback_period_size {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "capture and playback negotiated different period sizes ({capture_period_size} vs {playback_period_size} frames)"
                ),
            ));
        }

        capture_handle.prepare()?;
        playback_handle.prepare()?;

        if capture_handle.count() == 0 || playback_handle.count() == 0 {
            return Err(ErrorKind::DeviceNotAvailable.into());
        }

        let (capture_creation_ts, capture_timestamp_mode) =
            creation_timestamp(&capture_handle, capture_hw_params)?;
        let (playback_creation_ts, playback_timestamp_mode) =
            creation_timestamp(&playback_handle, playback_hw_params)?;

        let period_size = capture_period_size as usize;
        let capture_frame_size = frame_size(input_sample_format, config.input_channels);
        let playback_frame_size = frame_size(output_sample_format, config.output_channels);

        let stream_inner = DuplexStreamInner {
            control: WorkerControl::default(),
            capture: DuplexCaptureState {
                handle: capture_handle,
                sample_format: input_sample_format,
                frame_size: capture_frame_size,
                period_samples: period_size * config.input_channels as usize,
                timestamp_mode: capture_timestamp_mode,
                creation_ts: capture_creation_ts,
            },
            playback: DuplexPlaybackState {
                handle: playback_handle,
                sample_format: output_sample_format,
                frame_size: playback_frame_size,
                period_samples: period_size * config.output_channels as usize,
                timestamp_mode: playback_timestamp_mode,
                creation_ts: playback_creation_ts,
                equilibrium: EquilibriumFill::new(
                    output_sample_format,
                    period_size * playback_frame_size,
                ),
            },
            sample_rate: config.sample_rate,
            period_size,
            linked: AtomicBool::new(false),
            creation_instant: std::time::Instant::now(),
            pending_xrun: AtomicBool::new(false),
            _context: self._context.clone(),
        };

        Ok(stream_inner)
    }

    fn description(&self) -> Result<DeviceDescription, Error> {
        let name = self
            .desc
            .as_ref()
            .and_then(|desc| desc.lines().next())
            .unwrap_or(self.pcm_id.as_str());

        let mut builder = DeviceDescriptionBuilder::new(name)
            .driver(self.pcm_id.as_str())
            .direction(self.direction);

        if let Some(ref desc) = self.desc {
            builder = builder.extended(desc.lines().map(|l| l.trim()).filter(|l| !l.is_empty()));
        }

        Ok(builder.build())
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId::new(crate::platform::HostId::Alsa, &self.pcm_id))
    }

    fn supported_configs(
        &self,
        stream_t: alsa::Direction,
    ) -> Result<VecIntoIter<SupportedStreamConfigRange>, Error> {
        let pcm = open_pcm(&self.pcm_id, stream_t)?;

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

        let min_rate = hw_params.get_rate_min()?;
        let max_rate = hw_params.get_rate_max()?;

        let sample_rates = if min_rate == max_rate || hw_params.test_rate(min_rate + 1).is_ok() {
            // Fixed rate or continuous range.
            vec![(min_rate, max_rate)]
        } else {
            // Discrete rates: probe the standard list plus the hardware's own min and max so
            // that rates outside `COMMON_SAMPLE_RATES` are not missed.
            let mut probe: Vec<SampleRate> = COMMON_SAMPLE_RATES.to_vec();
            probe.push(min_rate);
            probe.push(max_rate);
            probe.sort_unstable();
            probe.dedup();
            probe
                .into_iter()
                .filter(|&r| (min_rate..=max_rate).contains(&r) && hw_params.test_rate(r).is_ok())
                .map(|r| (r, r))
                .collect()
        };

        let min_channels = hw_params.get_channels_min()?;
        // 64 = AES10 (MADI) maximum; also prevents spinning on plugins like plughw that report u32::MAX.
        const CHANNEL_ENUM_CAP: u32 = 64;
        let max_channels = hw_params
            .get_channels_max()?
            .min(CHANNEL_ENUM_CAP)
            .min(ChannelCount::MAX as u32);

        let supported_channels: Vec<ChannelCount> =
            if min_channels == max_channels || hw_params.test_channels(min_channels + 1).is_ok() {
                (min_channels..=max_channels)
                    .map(|c| c as ChannelCount)
                    .collect()
            } else {
                (min_channels..=max_channels)
                    .filter(|&c| hw_params.test_channels(c).is_ok())
                    .map(|c| c as ChannelCount)
                    .collect()
            };

        let mut output =
            Vec::with_capacity(FORMATS.len() * supported_channels.len() * sample_rates.len());
        let mut seen_formats: Vec<SampleFormat> = Vec::with_capacity(FORMATS.len());

        // Key: (channels, physical width in bits) with 4 physical widths (8/16/32/64 bits)
        let mut buffer_size_cache: HashMap<(ChannelCount, u32), SupportedBufferSize> =
            HashMap::with_capacity(supported_channels.len() * 4);

        for &(sample_format, alsa_format) in FORMATS.iter() {
            if seen_formats.contains(&sample_format) || hw_params.test_format(alsa_format).is_err()
            {
                continue;
            }
            seen_formats.push(sample_format);
            let width = alsa_format.physical_width().unwrap_or(0) as u32;

            for &channels in &supported_channels {
                let buffer_size =
                    *buffer_size_cache
                        .entry((channels, width))
                        .or_insert_with(|| {
                            supported_period_size_range(&hw_params, alsa_format, channels)
                        });

                for &(min_rate, max_rate) in sample_rates.iter() {
                    output.push(SupportedStreamConfigRange {
                        channels,
                        min_sample_rate: min_rate,
                        max_sample_rate: max_rate,
                        buffer_size,
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
        let mut formats: Vec<_> = self.supported_configs(stream_t)?.collect();

        formats.sort_by(|a, b| a.cmp_default_heuristics(b));

        match formats.into_iter().next_back() {
            Some(f) => Ok(f
                .try_with_standard_sample_rate()
                .unwrap_or_else(|| f.with_max_sample_rate())),
            None => Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                "No supported configuration",
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

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.pcm_id == other.pcm_id
    }
}

impl Eq for Device {}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = self.description().map_err(|_| fmt::Error)?;
        f.write_str(desc.name())
    }
}

impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pcm_id.hash(state);
    }
}

fn frame_size(sample_format: SampleFormat, channels: ChannelCount) -> usize {
    sample_format.sample_size() * channels as usize
}
