//! OSS devices: enumeration via /dev/sndstat.

use std::{fs, io, vec};

use crate::{DeviceDirection, Error, traits::DeviceTrait};

use super::{DEFAULT_DSP_PATH, SNDSTAT_PATH, error_from_io};

#[derive(Clone, Debug)]
pub struct Device {
    /// The full path to the device node to open (e.g. "/dev/dsp0" or "/dev/dsp")
    pub(super) path: String,
    /// Human-readable name from sndstat, (e.g. "Intel 82801I (ICH9) High Definition Audio Controller")
    pub(super) desc: Option<String>,
    pub(super) direction: DeviceDirection,
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = Vec<crate::SupportedStreamConfig>;
    type SupportedOutputConfigs = Vec<crate::SupportedStreamConfig>;
    type Stream = super::Stream;

    fn description(&self) -> Result<Option<String>, Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn id(&self) -> Result<crate::DeviceId, Error> {
        Ok(crate::DeviceId::new(crate::HostId::OSS, self.path.clone()))
    }

    fn supports_duplex(&self) -> bool {
        // todo: SNDCTL_DSP_GETCAPS can be used to actually
        // check for true duplex as opposed to simplex
        false
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn default_input_config(&self) -> Result<crate::SupportedStreamConfig, Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn default_output_config(&self) -> Result<crate::SupportedStreamConfig, Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: crate::StreamConfig,
        sample_format: crate::SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&crate::Data, &crate::CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static
    {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: crate::StreamConfig,
        sample_format: crate::SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut crate::Data, &crate::CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static
    {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }
}

/// Return the default device.
/// If the /dev/dsp virtual node is available, always return that,
/// otherwise fallback to the first device, otherwise, None.
pub fn default_device() -> Option<Device> {
    if std::path::Path::new(DEFAULT_DSP_PATH).exists() {
        Some(Device {
            path: DEFAULT_DSP_PATH.to_owned(),
            desc: Some("Default Audio Device".to_owned()),
            direction: DeviceDirection::Unknown,
        })
    } else {
        // [todo] enumerate devices and return the first one, if any
        None
    }
}

/// the virtual default node, opens nothing so capabilities are unknown until it is opened
// [TODO]: is this always present? I assumed not.
pub fn default_device() -> Self {
    Self {
        path: DEFAULT_DSP_PATH.to_owned(),
        desc: Some("Default Audio Device".to_owned()),
        direction: DeviceDirection::Unknown,
    }
}

pub struct Devices {
    inner: vec::IntoIter<Device>,
}

impl Devices {
    /// if the file is missing, assume OSS is unavailable, as /dev/sndstat is created by sound(4) on creationg
    /// do not assume no devices, as that forms a valid sndstat file
    pub fn new() -> Result<Self, Error> {
        let devices = match fs::read(SNDSTAT_PATH) {
            Ok(bytes) => parse_sndstat(&String::from_utf8_lossy(&bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Error::HostNotAvailable),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(Error::PermissionDenied),
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