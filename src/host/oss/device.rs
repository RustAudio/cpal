//! OSS device enumeration via /dev/sndstat.

use std::{fs, io, vec, fmt, sync::Arc, fmt::Display};

use crate::{
    host::{latch::Latch, oss::stream},
    SupportedStreamConfigRange,
    DeviceDescriptionBuilder,
    validate_stream_config,
    SupportedStreamConfig,
    traits::DeviceTrait,
    DuplexCallbackInfo,
    DuplexStreamConfig,
    DeviceDescription,
    DeviceDirection,
    StreamConfig,
    CallbackInfo,
    SampleFormat,
    ErrorKind,
    Error,
    Data,
};

use super::{
    syscall::{self, ioctl_int},
    DEFAULT_DSP_PATH,
    SNDSTAT_PATH,
    stream::Control,
    error_from_io,
    probe::{
        probe_configs,
        open_dsp,
        Mode,
    },
};

#[derive(Clone, Debug, Hash)]
pub struct Device {
    /// The full path to the device node to open (e.g. "/dev/dsp0" or "/dev/dsp")
    pub(super) path: String,
    /// Human-readable name from sndstat, (e.g. "Intel 82801I (ICH9) High Definition Audio Controller")
    pub(super) desc: Option<String>, // [TODO]: Maybe not Option?
    pub(super) direction: DeviceDirection,
}

// Every single host seems to have this exact same Display code
impl Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Instead of directly using self.path here, I'm going to leave it as is, for when I get round to
        // proper device names.
        let desc = self.description().map_err(|_| fmt::Error)?;
        f.write_str(desc.name())
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Device {}

#[derive(Clone)]
pub struct SupportedConfigsIter {
    inner: vec::IntoIter<SupportedStreamConfigRange>,
}

impl Iterator for SupportedConfigsIter {
    type Item = SupportedStreamConfigRange;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedConfigsIter;
    type SupportedOutputConfigs = SupportedConfigsIter;
    type Stream = super::Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        let name = self.desc.as_ref().unwrap_or(&self.path);
        let builder = DeviceDescriptionBuilder::new(name)
            .address(&self.path)
            .direction(self.direction);
            // .extended(lines); extra lines from /dev/sndstat

        Ok(builder.build())
    }

    // [TODO]: Strip /dev/ from each path
    fn id(&self) -> Result<crate::DeviceId, Error> {
        // I don't think this is persistent between changing devices
        // infalliable though
        Ok(crate::DeviceId::new(crate::HostId::Oss, self.path.clone()))
    }

    fn supports_input(&self) -> bool {
        matches!(self.direction, DeviceDirection::Input | DeviceDirection::Duplex)
    }
    
    fn supports_output(&self) -> bool {
        matches!(self.direction, DeviceDirection::Output | DeviceDirection::Duplex)
    }

    /// Whether or whether not the driver reports true native duplex for this device.
    /// Any failure to open or query means returns false.
    /// 
    /// [NOTE]: This currently does not cache the result, and performs a syscall everytime.
    fn supports_duplex(&self) -> bool {
        let Ok(fd) = open_dsp(&self.path, Mode::ReadWrite) else { return false };
        let Ok(caps) = ioctl_int(&fd, syscall::SNDCTL_DSP_GETCAPS, 0) else { return false };
        caps & syscall::PCM_CAP_DUPLEX != 0
        // [TODO]: make self.direction mutable (with only &self?) and set here
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        if self.supports_input() {
            match probe_configs(&self.path, Mode::Read) {
                Ok(ranges) => Ok(SupportedConfigsIter { inner: ranges.into_iter() }),
                Err(e) => Err(error_from_io(e)),
            }
        } else {
            Err(Error::new(ErrorKind::UnsupportedOperation))
        }
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        if self.supports_output() {
            match probe_configs(&self.path, Mode::Write) {
                Ok(ranges) => Ok(SupportedConfigsIter { inner: ranges.into_iter() }),
                Err(e) => Err(error_from_io(e)),
            }
        } else {
            Err(Error::new(ErrorKind::UnsupportedOperation))
        }
    }

    // [TODO]: best? or research probing a specific format with `SNDCTL_DSP_GETCAPS`
    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        if self.supports_input() {
            self.supported_input_configs()?
                .next()
                .map(|range| range.with_max_sample_rate())
                .ok_or(Error::new(ErrorKind::UnsupportedConfig))
        } else {
            Err(Error::new(ErrorKind::UnsupportedOperation))
        }
    }

    // this isn't the best approach atall, it can spew out exotic configs as 'default',
    // e.g., on my headphones: 1ch 192kHz, Unkown (though thats due to my probing), F32 (fine)
    //
    // research: maybe `AUDIOINFO` has something for this?
    // 2ch 48khz 1024 f32 would be far more sane
    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        if self.supports_output() {
            self.supported_output_configs()?
                .next()
                .map(|range| range.with_max_sample_rate())
                .ok_or(Error::new(ErrorKind::UnsupportedConfig))
        } else {
            Err(Error::new(ErrorKind::UnsupportedOperation))
        }
    }

    // Leaving this here as a reminder for me to look into this,
    // havent read through the default fn in DeviceTrait, but I
    // doubt it will work properly due to OSS duplex limitations
    // fn default_duplex_config(&self) -> Result<DuplexStreamConfig, Error> {
    //     Err(Error::from(ErrorKind::HostUnavailable))
    // }

    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error               ) + Send + 'static
    {
        validate_stream_config(&config)?;

        // Open and configure first, before dispatching to the worker thread, so errors can be returned here
        let fd = open_dsp(&self.path, Mode::Read).map_err(error_from_io)?; // diff from output
        let (period_bytes, buf_size) = stream::configure(&fd, &config, sample_format, true)?;

        let control = Arc::new(Control::default());
        let worker_ctl = control.clone();

        let worker = std::thread::Builder::new()
            .name("cpal_oss_in".into()) // diff from output
            .spawn(move || {
                stream::input_loop(fd, worker_ctl, period_bytes, data_callback, error_callback, sample_format); // diff from output
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to create OSS input worker thread: {e}"),
                )
            })?;

        Ok(super::Stream { reported_buffer_size: Some(buf_size), control, worker: Some(worker), latch: Latch::new() })
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error                   ) + Send + 'static
    {
        validate_stream_config(&config)?;

        // Open and configure first, before dispatching to the worker thread, so errors can be returned here
        let fd = open_dsp(&self.path, Mode::Write).map_err(error_from_io)?;
        let (period_bytes, buf_size) = stream::configure(&fd, &config, sample_format, false)?;

        let control = Arc::new(Control::default());
        let worker_ctl = control.clone();

        let worker = std::thread::Builder::new()
            .name("cpal_oss_out".into())
            .spawn(move || {
                stream::output_loop(fd, worker_ctl, period_bytes, data_callback, error_callback, sample_format);
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to create OSS output worker thread: {e}"),
                )
            })?;

        Ok(super::Stream { reported_buffer_size: Some(buf_size), control, worker: Some(worker), latch: Latch::new() })
    }

    // [NOTE]: OSS duplex streams must have the same input and output sample format,
    // this is a limitation of the host itself.
    fn build_duplex_stream_raw<D, E>(
        &self,
        config: DuplexStreamConfig,
        input_sample_format: SampleFormat,
        output_sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error                                ) + Send + 'static,
    {
        validate_duplex_stream_config(&config)?;
        let sample_format = if input_sample_format == output_sample_format {
            input_sample_format
        } else {
            return Err(Error::with_message(ErrorKind::UnsupportedConfig, "OSS duplex must share the sample sample format between input and output"));
        };

        // Open and configure first, before dispatching to the worker thread, so errors can be returned here
        let fd = open_dsp(&self.path, Mode::ReadWrite).map_err(error_from_io)?;
        let (in_period_bytes, out_period_bytes) = stream::configure_duplex(&fd, &config, sample_format)?;

        let control = Arc::new(Control::default());
        let worker_ctl = control.clone();

        let worker = std::thread::Builder::new()
            .name("cpal_oss_dup".into())
            .spawn(move || {
                stream::duplex_loop(fd, worker_ctl, in_period_bytes, out_period_bytes, data_callback, error_callback, sample_format);
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to create OSS duplex worker thread: {e}"),
                )
            })?;

        Ok(super::Stream { reported_buffer_size: None, control, worker: Some(worker), latch: Latch::new() })
    }
}

fn validate_duplex_stream_config(config: &DuplexStreamConfig) -> Result<(), Error> {
    let input = StreamConfig {
        channels: config.input_channels,
        sample_rate: config.sample_rate,
        buffer_size: config.buffer_size,
    };
    validate_stream_config(&input)?;

    let output = StreamConfig {
        channels: config.output_channels,
        sample_rate: config.sample_rate,
        buffer_size: config.buffer_size,
    };
    validate_stream_config(&output)
}

/// Return the default device.
/// If the /dev/dsp virtual node is available, always return that,
/// otherwise fallback to the first device, otherwise, None.
pub fn default_device() -> Option<Device> {
    if std::path::Path::new(DEFAULT_DSP_PATH).exists() {
        Some(Device {
            path: DEFAULT_DSP_PATH.to_owned(),
            desc: Some("Default Audio Device".to_owned()),
            direction: detect_direction(DEFAULT_DSP_PATH),
        })
    } else {
        // [todo] enumerate devices and return the first one, if any
        None
    }
}

/// **Note**: This could fail when the device is busy, which is suboptimal, [TODO]
fn detect_direction(path: &str) -> DeviceDirection {
    match (open_dsp(path, Mode::Read), open_dsp(path, Mode::Write)) {
        (Ok(_),  Ok(_) ) => DeviceDirection::Duplex,
        (Ok(_),  Err(_)) => DeviceDirection::Input,
        (Err(_), Ok(_) ) => DeviceDirection::Output,
        _ => DeviceDirection::Unknown,
    }
}

pub struct Devices {
    inner: vec::IntoIter<Device>,
}

impl Devices {
    /// If the file is missing, assume OSS is unavailable, as /dev/sndstat is created by sound(4) on creationg
    /// do not assume no devices, as that forms a valid sndstat file
    /// 
    /// [TODO]: Remove this and use `ossinfo`
    /// [NOTE]: This is formally obsolete now, and exists for compatability.
    /// [DOCS]: [Depreciation notice and description](http://manuals.opensound.com/usersguide/sndstat.html)
    /// [DOCS]: [IOCTL to replace this](https://man.freebsd.org/cgi/man.cgi?manpath=FreeBSD+14.1-RELEASE+and+Ports&query=sndstat&sektion=4#1)
    pub fn new() -> Result<Self, Error> {
        let devices = match fs::read(SNDSTAT_PATH) {
            Ok(bytes) => parse_sndstat(&String::from_utf8_lossy(&bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Error::new(ErrorKind::DeviceNotAvailable)),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(Error::new(ErrorKind::PermissionDenied)),
            Err(e) => return Err(error_from_io(e)),
        };
        Ok(Self {
            inner: devices.into_iter(),
        })
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

fn parse_sndstat(text: &str) -> Vec<Device> {
    text.lines().filter_map(parse_line).collect()
}

/// Parses a line like:
///   pcm0: <Intel 82801I (ICH9) High Definition Audio Controller> (play/rec) default
///
/// Header lines (e.g., "Installed devices:") and indented/verbose detail lines which don't start
/// with "pcmX:" and are skipped
fn parse_line(line: &str) -> Option<Device> {
    let rest = line.strip_prefix("pcm")?;
    let (unit, rest) = rest.split_once(':')?;
    let unit: u32 = unit.parse().ok()?;

    // Description is the text inside the first <...>.
    let desc = rest.find('<').and_then(|start| {
        let inner = &rest[start + 1..];
        inner.find('>').map(|end| inner[..end].to_owned())
    });

    // Everything after the description holds the capability flags
    let tail = rest.rfind('>').map_or(rest, |i| &rest[i + 1..]);
    let can_play = tail.contains("play");
    let can_rec = tail.contains("rec");
    let direction = match (can_play, can_rec) {
        (true, true) => DeviceDirection::Duplex,
        (true, false) => DeviceDirection::Output,
        (false, true) => DeviceDirection::Input,
        // impossible?
        (false, false) => DeviceDirection::Unknown,
    };

    Some(Device {
        path: format!("/dev/dsp{unit}"),
        desc,
        direction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sndstat() {
        let text = "Installed devices:\n\
                    pcm0: <Intel 82801I (ICH9) High Definition Audio Controller> (play/rec) default\n\
                    pcm1: <USB audio> (play)\n";
        let devs = parse_sndstat(text);
        assert_eq!(devs.len(), 2);
        assert_eq!(devs[0].path, "/dev/dsp0");
        assert_eq!(devs[1].path, "/dev/dsp1");
        assert_eq!(devs[1].desc.as_deref(), Some("USB audio"));
    }

    #[test]
    fn no_devices() {
        assert!(parse_sndstat("Installed devices:\nNo devices installed.\n").is_empty());
    }
}