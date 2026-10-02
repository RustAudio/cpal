use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Config {
    pub format  : SampleFormat,
    pub channels: u16,
    pub input   : bool,
}
