use crate::ErrorKind::*;
use crate::*;
use azo::Driver;
use azo::dto::*;
use azo::sys::*;
use std::borrow::Cow;
use std::ffi::c_void;
use std::mem;

pub type CpalResult<T> = Result<T, Error>;

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
/// workaround until `#![feature(type_alias_impl_trait)]` is stabilized
#[macro_export]
macro_rules! context_type {
    () => { Context<data_cb_type!(), error_cb_type!()> };
}
/// workaround until `#![feature(type_alias_impl_trait)]` is stabilized
#[macro_export]
macro_rules! context_handle_type {
    () => { Arc<Mutex<context_type!()>> };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// just to make the pointers `Send`
pub struct DoubleBuffer(pub [*mut c_void; 2]);

unsafe impl Send for DoubleBuffer {}
unsafe impl Sync for DoubleBuffer {}

use crate::SampleFormat as CpalFormat;
use azo::sys::SampleType as AsioFormat;

pub const fn sample_format_asio2cpal(asio_format: AsioFormat) -> Option<CpalFormat> {
    // FIXME: consider using `cfg_select!` here once the MSRV has risen to 1.95+
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
    Err(Error::with_message(kind, message))
}

pub fn create_report(driver: &Driver, asio_error: azo::Error, origin: &str) -> Error {
    let last_error = driver.last_error();

    Error::with_message(
        BackendError,
        format!("[ASIO] {origin}() failed with `{asio_error}` - {last_error:?}"),
    )
}

pub fn create_minimal_asio_time(pos: &SamplePosition) -> Time {
    Time {
        time_info: TimeInfo {
            system_time: pos.time_stamp,
            sample_position: pos.position,
            flags: TimeInfoFlags::SYSTEM_TIME_VALID | TimeInfoFlags::SAMPLE_POSITION_VALID,
            ..unsafe { mem::zeroed() }
        },
        ..unsafe { mem::zeroed() }
    }
}
