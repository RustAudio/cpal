use crate::ErrorKind::*;
use crate::*;
use azo::dto::*;
use azo::{Driver, sys::*};
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
/// just to make the pointers `Send`
pub struct DoubleBuffer(pub [*mut c_void; 2]);

unsafe impl Send for DoubleBuffer {}
unsafe impl Sync for DoubleBuffer {}

use crate::SampleFormat as CpalFormat;
use azo::sys::SampleType as AzoFormat;

pub const fn sample_format_azo2cpal(azo_format: AzoFormat) -> Option<CpalFormat> {
    cfg_select! {
        target_endian = "little" => {
            const AZO_I16   : AzoFormat = AzoFormat::PCM_I16_LSB;
            const AZO_I24   : AzoFormat = AzoFormat::PCM_I32_LSB_24;
            const AZO_I32   : AzoFormat = AzoFormat::PCM_I32_LSB;
            const AZO_F32   : AzoFormat = AzoFormat::PCM_F32_LSB;
            const AZO_DSD_U8: AzoFormat = AzoFormat::DSD_I8_LSB_1;
        },
        target_endian = "big" => {
            const AZO_I16   : AzoFormat = AzoFormat::PCM_I16_MSB;
            const AZO_I24   : AzoFormat = AzoFormat::PCM_I32_MSB_24;
            const AZO_I32   : AzoFormat = AzoFormat::PCM_I32_MSB;
            const AZO_F32   : AzoFormat = AzoFormat::PCM_F32_MSB;
            const AZO_DSD_U8: AzoFormat = AzoFormat::DSD_I8_MSB_1;
        }
    }

    match azo_format {
        AZO_I16    => Some(CpalFormat::I16),
        AZO_I24    => Some(CpalFormat::I24),
        AZO_I32    => Some(CpalFormat::I32),
        AZO_F32    => Some(CpalFormat::F32),
        AZO_DSD_U8 => Some(CpalFormat::DsdU8),

        _ => None // no matching counterpart in cpal
    }
}

/// just for convenience
pub fn err<T>(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> CpalResult<T> {
    Err(Error::with_message(kind, message))
}

pub fn create_report(driver: &Driver, azo_error: azo::Error, origin: &str) -> Error {
    let last_error = driver.last_error();

    Error::with_message(
        BackendError,
        format!(".{origin}() failed with `{azo_error}` - {last_error:?}"),
    )
}

pub fn create_minimal_azo_time(pos: &SamplePosition) -> Time {
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
