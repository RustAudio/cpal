use crate::{transcoder, Endianness, SampleFormat};

use super::{FromBytes, ToBytes, BIG_ENDIAN, LITTLE_ENDIAN};

type Sample = u32;
const BYTES: usize = 4;

impl ToBytes<BYTES, LITTLE_ENDIAN> for u32 {
    #[inline]
    fn to_bytes(self) -> [u8; BYTES] {
        self.to_le_bytes()
    }
}
impl ToBytes<BYTES, BIG_ENDIAN> for u32 {
    #[inline]
    fn to_bytes(self) -> [u8; BYTES] {
        self.to_be_bytes()
    }
}
impl FromBytes<BYTES, LITTLE_ENDIAN> for u32 {
    #[inline]
    fn from_bytes(bytes: [u8; BYTES]) -> Self {
        Self::from_le_bytes(bytes)
    }
}
impl FromBytes<BYTES, BIG_ENDIAN> for u32 {
    #[inline]
    fn from_bytes(bytes: [u8; BYTES]) -> Self {
        Self::from_be_bytes(bytes)
    }
}

pub struct B4LE {}
transcoder!(
    B4LE,
    Sample,
    BYTES,
    LITTLE_ENDIAN,
    SampleFormat::U32B4(Endianness::Little)
);

pub struct B4BE {}
transcoder!(
    B4BE,
    Sample,
    BYTES,
    BIG_ENDIAN,
    SampleFormat::U32B4(Endianness::Big)
);

#[cfg(target_endian = "big")]
pub type B4NE = B4BE;
#[cfg(target_endian = "little")]
pub type B4NE = B4LE;
