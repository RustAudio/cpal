//! OSS backend for FreeBSD.
//!
//! FreeBSD's `sound(4)` implements the OSS API in the kernel (`/dev/dspN`), so there is no
//! userspace library or plugin layer to initialize, unlike ALSA.

use std::{io, path::Path};

use crate::{
    DeviceId, Error, ErrorKind,
    traits::{DeviceTrait, HostTrait},
};

pub use self::device::{Device, Devices};
pub use self::stream::Stream;

mod device;
mod stream;

/// Kernel routes this node to the default unit (`hw.snd.default_unit`).
const DEFAULT_DSP_PATH: &str = "/dev/dsp";

/// Present whenever `sound(4)` is loaded, even if there are no sound cards present
const SNDSTAT_PATH: &str = "/dev/sndstat";

/// The FreeBSD OSS host type.
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, Error> {
        Ok(Self)
    }

    fn default_device(&self) -> Option<Device> {
        if !Path::new(DEFAULT_DSP_PATH).exists() {
            return None;
        }
        Some(Device::default_device())
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // sound(4) is part of the FreeBSD base system, however it may not be loaded
        // (e.g., a custom kernel), so check for its control node.
        Path::new(SNDSTAT_PATH).exists()
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Devices::new()
    }

    fn device_by_id(&self, id: &DeviceId) -> Option<Self::Device> {
        let canonical_id = DeviceId::new(id.host(), canonical_dsp_path(id.id()));
        self.devices()
            .ok()?
            .find(|d| d.id().ok().as_ref() == Some(&canonical_id))
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.default_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.default_device()
    }
}

/// Normalize a device name to a full path
/// "dsp0"` (or) "/dev/dsp0" -> "/dev/dsp0"
fn canonical_dsp_path(id: &str) -> String {
    if id.starts_with('/') {
        id.to_owned()
    } else {
        format!("/dev/{id}")
    }
}

/// Map an OS error from `open`/`ioctl`/`read`/`write` on a dsp node to a `cpal::Error`
#[allow(dead_code)]
fn error_from_io(err: io::Error) -> Error {
    match err.raw_os_error() {
        Some(libc::ENODEV | libc::ENXIO | libc::ENOENT) => ErrorKind::DeviceNotAvailable.into(),
        Some(libc::EPERM | libc::EACCES) => ErrorKind::PermissionDenied.into(),
        Some(libc::EBUSY | libc::EAGAIN) => ErrorKind::DeviceBusy.into(),
        Some(libc::EINVAL) => ErrorKind::UnsupportedConfig.into(),
        Some(libc::ENOSYS | libc::ENOTSUP) => ErrorKind::UnsupportedOperation.into(),
        _ => Error::with_message(ErrorKind::BackendError, err.to_string()),
    }
}