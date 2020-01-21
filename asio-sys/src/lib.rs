#![allow(non_camel_case_types)]

#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;

#[allow(unused_imports)]
#[macro_use]
extern crate num_derive;
#[allow(unused_imports)]
extern crate num_traits;

#[cfg(asio)]
pub mod bindings;
#[cfg(asio)]
pub use bindings::errors::{AsioError, LoadDriverError};
#[cfg(asio)]
pub use bindings::*;
