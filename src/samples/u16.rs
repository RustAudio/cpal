use crate::{transcoder, SampleFormat, Endianness};

use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN};

type Sample = u16;
const BYTES: usize = 2;

impl ToBytes<BYTES, LITTLE_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; BYTES] { self.to_le_bytes() } }
impl ToBytes<BYTES, BIG_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; BYTES] { self.to_be_bytes() } }
impl FromBytes<BYTES, LITTLE_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; BYTES]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<BYTES, BIG_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; BYTES]) -> Self { Self::from_be_bytes(bytes) } }

pub struct B2LE {}
transcoder!(B2LE, Sample, BYTES, LITTLE_ENDIAN, SampleFormat::U16B2(Endianness::Little));

pub struct B2BE {}
transcoder!(B2BE, Sample, BYTES, BIG_ENDIAN, SampleFormat::U16B2(Endianness::Big));

#[cfg(target_endian = "big")]
pub type B2NE = B2BE;
#[cfg(target_endian = "little")]
pub type B2NE = B2LE;
