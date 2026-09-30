use crate::ErrorKind;
use std::borrow::Cow;

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

/// just for convenience
pub fn err<T>(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> CpalResult<T> {
    Err(crate::Error::with_message(kind, message))
}
