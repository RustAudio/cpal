use crate::ErrorKind::*;
use crate::{DeviceDirection, ErrorKind, StreamInstant, SupportedBufferSize};
use azo::driver::Driver;
use azo::dto::{BufferSize, ChannelCounts};
use parking_lot::{Condvar, Mutex};
use std::borrow::Cow;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

pub type CpalResult<T> = Result<T, crate::Error>;

/// workaround until `#![feature(type_alias_impl_trait)]` is stabilized
#[macro_export]
macro_rules! data_cb_type {
    () => { impl FnMut(&$crate::Data, &mut $crate::Data, &$crate::DuplexCallbackInfo) + Send + 'static }
}
/// workaround until `#![feature(type_alias_impl_trait)]` is stabilized
#[macro_export]
macro_rules! error_cb_type {
    () => { impl FnMut($crate::Error) + Send + 'static };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// just to make the pointers `Send`
pub struct DoubleBuffer(pub [*mut c_void; 2]);

unsafe impl Send for DoubleBuffer {}

use crate::SampleFormat as CpalFormat;
use azo::sys::SampleType as AsioFormat;

pub const fn sample_format_asio2cpal(asio_format: AsioFormat) -> Option<CpalFormat> {
    const BIG_ENDIAN: bool = cfg!(target_endian = "big");
    const PCM_I16: AsioFormat = if BIG_ENDIAN { AsioFormat::PCM_I16_MSB    } else { AsioFormat::PCM_I16_LSB    };
    const PCM_I24: AsioFormat = if BIG_ENDIAN { AsioFormat::PCM_I32_MSB_24 } else { AsioFormat::PCM_I32_LSB_24 };
    const PCM_I32: AsioFormat = if BIG_ENDIAN { AsioFormat::PCM_I32_MSB    } else { AsioFormat::PCM_I32_LSB    };
    const PCM_F32: AsioFormat = if BIG_ENDIAN { AsioFormat::PCM_F32_MSB    } else { AsioFormat::PCM_F32_LSB    };
    const DSD_U8 : AsioFormat = if BIG_ENDIAN { AsioFormat::DSD_I8_MSB_1   } else { AsioFormat::DSD_I8_LSB_1   };

    #[deny(nonstandard_style, reason = "prevent accidental wildcard patterns")]
    match asio_format {
        PCM_I16 => Some(CpalFormat::I16),
        PCM_I24 => Some(CpalFormat::I24),
        PCM_I32 => Some(CpalFormat::I32),
        PCM_F32 => Some(CpalFormat::F32),
        DSD_U8  => Some(CpalFormat::DsdU8),

        _ => None, // no matching counterpart in cpal
    }
}

/// just for convenience
pub fn err<T>(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> CpalResult<T> {
    Err(crate::Error::with_message(kind, message))
}

impl From<BufferSize> for SupportedBufferSize {
    fn from(value: BufferSize) -> Self {
        Self::Range {
            min: value.min as _,
            max: value.max as _,
        }
    }
}

impl From<ChannelCounts> for DeviceDirection {
    fn from(value: ChannelCounts) -> Self {
        match value {
            ChannelCounts { in_: 1.., out: 1.. } => Self::Duplex,
            ChannelCounts { in_: 1.., out: 0   } => Self::Input,
            ChannelCounts { in_: 0  , out: 1.. } => Self::Output,
            _                                    => Self::Unknown,
        }
    }
}

pub trait Decorate {
    type OkValue;
    fn decorate(self, driver: &impl Driver, method_name: &'static str) -> CpalResult<Self::OkValue>;
}

impl<T> Decorate for azo::Result<T> {
    type OkValue = T;
    fn decorate(self, driver: &impl Driver, function_name: &'static str) -> CpalResult<Self::OkValue> {
        let azo_error = match self {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };

        let last_error = driver.last_error();

        err(BackendError, format!("[ASIO] {function_name}() failed with `{azo_error}` - {last_error:?}"))
    }
}

#[derive(Debug, Default)]
pub struct Barrier {
    mutex: Mutex<()>,
    condvar: Condvar,
}

impl Barrier {
    pub fn wait(&self, deadline: Option<Instant>) -> CpalResult<()> {
        let mut guard = self.mutex.lock();

        let timed_out = if let Some(timeout) = deadline {
            self.condvar.wait_until(&mut guard, timeout).timed_out()
        } else {
            self.condvar.wait(&mut guard);
            false
        };

        if timed_out {
            return err(ResourceExhausted, "timeout exceeded while draining");
        }

        Ok(())
    }

    pub fn notify(&self) {
        self.condvar.notify_all();
    }
}

#[derive(Debug, Default)]
pub struct Monotonizer {
    overflows: AtomicUsize,
    latest: AtomicU64,
}

impl Monotonizer {
    /// The spec requires timestamps to be derived from `timeGetTime()` on Windows,
    /// which overflows every every ~50 days.
    pub fn monotonize(&self, raw: u64) -> StreamInstant {
        let previous = self.latest.swap(raw, Ordering::Relaxed);

        let overflows =
            if raw < previous { self.overflows.fetch_add(1, Ordering::Relaxed) + 1 }
            else { self.overflows.load(Ordering::Relaxed) };

        Self::adjust_value(raw, overflows)
    }

    pub fn latest(&self) -> StreamInstant {
        let raw = self.latest.load(Ordering::Relaxed);
        let overflows = self.overflows.load(Ordering::Relaxed);

        Self::adjust_value(raw, overflows)
    }

    const fn adjust_value(raw: u64, overflows: usize) -> StreamInstant {
        let offset = overflows as u64 * (u32::MAX as u64 + 1);
        StreamInstant::from_nanos(raw + offset)
    }
}
