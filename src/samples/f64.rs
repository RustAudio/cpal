use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN, NATIVE_ENDIAN};

impl ToBytes<8, LITTLE_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_le_bytes() } }
impl ToBytes<8, BIG_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_be_bytes() } }
impl ToBytes<8, NATIVE_ENDIAN> for f64 { #[inline] fn to_bytes(self) -> [u8; 8] { self.to_ne_bytes() } }
impl FromBytes<8, LITTLE_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<8, BIG_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<8, NATIVE_ENDIAN> for f64 { #[inline] fn from_bytes(bytes: [u8; 8]) -> Self { Self::from_ne_bytes(bytes) } }