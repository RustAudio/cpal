//! ALSA backend implementation.
//!
//! Default backend on Linux and BSD systems.

extern crate alsa;
#[cfg(feature = "realtime")]
extern crate alsa_sys;
extern crate libc;

use std::{
    mem,
    sync::{Arc, Mutex},
};

pub use self::device::Device;
pub use self::enumerate::Devices;
pub use self::stream::Stream;
use crate::{
    DeviceDirection, DeviceId, Error, ErrorKind,
    traits::{DeviceTrait, HostTrait},
};

mod device;
mod duplex;
mod enumerate;
mod hw_params;
mod stream;
mod timestamp;
mod trigger;
mod worker;

/// The default Linux and BSD host type.
#[derive(Debug, Clone)]
pub struct Host {
    inner: Arc<AlsaContext>,
}

impl Host {
    pub fn new() -> Result<Self, Error> {
        let inner = AlsaContext::new().map_err(|e| {
            Error::with_message(
                ErrorKind::HostUnavailable,
                format!("ALSA is not available: {e}"),
            )
        })?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    // "default" is a virtual ALSA device that redirects to the configured default. We cannot
    // determine its actual capabilities without opening it, so we return Unknown direction.
    fn default_device(&self) -> Device {
        Device {
            pcm_id: DEFAULT_DEVICE.to_owned(),
            desc: Some("Default Audio Device".to_owned()),
            direction: DeviceDirection::Unknown,
            _context: self.inner.clone(),
        }
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
        let canonical_id = DeviceId::new(id.host(), canonical_pcm_id(id.id()));
        self.devices()
            .ok()?
            .find(|d| d.id().ok().as_ref() == Some(&canonical_id))
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        Some(self.default_device())
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        Some(self.default_device())
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

const POLL_INFINITE: i32 = -1; // "block until an event arrives"
const TRIGGER_PAYLOAD_SIZE: libc::ssize_t = mem::size_of::<u64>() as libc::ssize_t;

// Some ALSA plugins (e.g. alsaequal, certain USB drivers) are not reentrant.
static ALSA_OPEN_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn open_pcm(pcm_id: &str, direction: alsa::Direction) -> Result<alsa::pcm::PCM, Error> {
    let _guard = ALSA_OPEN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    alsa::pcm::PCM::new(pcm_id, direction, true).map_err(|e| {
        let e = Error::from(e);
        if e.kind() == ErrorKind::UnsupportedConfig {
            let dir = match direction {
                alsa::Direction::Capture => "input",
                alsa::Direction::Playback => "output",
            };
            Error::with_message(
                ErrorKind::UnsupportedOperation,
                format!("Device does not support {dir}"),
            )
        } else {
            e
        }
    })
}

// TODO: Not yet defined in rust-lang/libc crate
const LIBC_ENOTSUPP: libc::c_int = 524;

fn canonical_pcm_id(pcm_id: &str) -> String {
    if let Some((prefix, rest)) = pcm_id.split_once(':') {
        let (card_str, device_str) = match rest.split_once(',') {
            Some((c, d)) => (c.trim(), d.trim()),
            None => (rest.trim(), "0"),
        };
        if card_str.contains('=') {
            if !rest.contains(',') {
                return format!("{prefix}:{rest},DEV=0");
            }
        } else if let Ok(device) = device_str.parse::<u32>() {
            return format!("{prefix}:CARD={card_str},DEV={device}");
        }
    }
    pcm_id.to_owned()
}

impl From<alsa::Error> for Error {
    fn from(err: alsa::Error) -> Self {
        match err.errno() {
            libc::ENODEV | libc::ENOENT | LIBC_ENOTSUPP => ErrorKind::DeviceNotAvailable.into(),
            libc::EPERM | libc::EACCES => ErrorKind::PermissionDenied.into(),
            libc::EBUSY | libc::EAGAIN => ErrorKind::DeviceBusy.into(),
            libc::EINVAL => ErrorKind::UnsupportedConfig.into(),
            libc::ENOSYS => ErrorKind::UnsupportedOperation.into(),
            _ => Error::with_message(ErrorKind::BackendError, err.to_string()),
        }
    }
}
