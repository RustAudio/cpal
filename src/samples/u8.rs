use crate::{transcoder, SampleFormat};

use super::{FromBytes, ToBytes, BIG_ENDIAN, LITTLE_ENDIAN};

type Sample = u8;
const BYTES: usize = 1;

// TODO remove endianness for this type alltogether
impl ToBytes<BYTES, LITTLE_ENDIAN> for u8 {
    #[inline]
    fn to_bytes(self) -> [u8; BYTES] {
        self.to_le_bytes()
    }
}
impl ToBytes<BYTES, BIG_ENDIAN> for u8 {
    #[inline]
    fn to_bytes(self) -> [u8; BYTES] {
        self.to_be_bytes()
    }
}
impl FromBytes<BYTES, LITTLE_ENDIAN> for u8 {
    #[inline]
    fn from_bytes(bytes: [u8; BYTES]) -> Self {
        Self::from_le_bytes(bytes)
    }
}
impl FromBytes<BYTES, BIG_ENDIAN> for u8 {
    #[inline]
    fn from_bytes(bytes: [u8; BYTES]) -> Self {
        Self::from_be_bytes(bytes)
    }
}

pub struct B1LE {}
transcoder!(B1LE, Sample, BYTES, LITTLE_ENDIAN, SampleFormat::U8B1);

pub struct B1BE {}
transcoder!(B1BE, Sample, BYTES, BIG_ENDIAN, SampleFormat::U8B1);

#[cfg(target_endian = "big")]
pub type B1NE = B1BE;
#[cfg(target_endian = "little")]
pub type B1NE = B1LE;
