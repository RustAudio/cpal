use std::fmt;
use std::fmt::Display;

pub mod containers;
pub mod server;
pub mod environment;

pub(crate) struct HexSlice<'a>(&'a [u8]);

impl<'a> Display for HexSlice<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &byte in self.0 {
            write!(f, "{:0>2x}", byte)?;
        }
        Ok(())
    }
}