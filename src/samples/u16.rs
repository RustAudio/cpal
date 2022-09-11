use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN, NATIVE_ENDIAN};

impl ToBytes<2, LITTLE_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_le_bytes() } }
impl ToBytes<2, BIG_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_be_bytes() } }
impl ToBytes<2, NATIVE_ENDIAN> for u16 { #[inline] fn to_bytes(self) -> [u8; 2] { self.to_ne_bytes() } }
impl FromBytes<2, LITTLE_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<2, BIG_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<2, NATIVE_ENDIAN> for u16 { #[inline] fn from_bytes(bytes: [u8; 2]) -> Self { Self::from_ne_bytes(bytes) } }
