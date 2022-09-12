use crate::{transcoder, SampleFormat, Endianness};

use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN};

type Sample = f64;
const BYTES: usize = 8;

impl ToBytes<BYTES, LITTLE_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; BYTES] { self.to_le_bytes() } }
impl ToBytes<BYTES, BIG_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; BYTES] { self.to_be_bytes() } }
impl FromBytes<BYTES, LITTLE_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; BYTES]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<BYTES, BIG_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; BYTES]) -> Self { Self::from_be_bytes(bytes) } }

pub struct B8LE {}
transcoder!(B8LE, Sample, BYTES, LITTLE_ENDIAN, SampleFormat::F64B8(Endianness::Little));

pub struct B8BE {}
transcoder!(B8BE, Sample, BYTES, BIG_ENDIAN, SampleFormat::F64B8(Endianness::Big));

#[cfg(target_endian = "big")]
pub type B8NE = B8BE;
#[cfg(target_endian = "little")]
pub type B8NE = B8LE;
