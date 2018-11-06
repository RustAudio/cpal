#![allow(non_camel_case_types)]

#[allow(unused_imports)]
#[macro_use]
extern crate lazy_static;

extern crate num;
#[allow(unused_imports)]
#[macro_use]
extern crate num_derive;

#[cfg(asio)]
pub mod bindings;
#[cfg(asio)]
pub use bindings::*;