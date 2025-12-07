#![allow(non_camel_case_types)]

#[macro_use]
extern crate num_derive;
extern crate num_traits;

#[cfg(asio)]
pub mod bindings;
#[cfg(asio)]
pub use bindings::errors::{AsioError, LoadDriverError};
#[cfg(asio)]
pub use bindings::*;
