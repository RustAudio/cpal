use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN, NATIVE_ENDIAN};

impl ToBytes<1, LITTLE_ENDIAN> for i8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_le_bytes() } }
impl ToBytes<1, BIG_ENDIAN> for i8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_be_bytes() } }
impl ToBytes<1, NATIVE_ENDIAN> for i8 { #[inline] fn to_bytes(self) -> [u8; 1] { self.to_ne_bytes() } }
impl FromBytes<1, LITTLE_ENDIAN> for i8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<1, BIG_ENDIAN> for i8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_be_bytes(bytes) } }
impl FromBytes<1, NATIVE_ENDIAN> for i8 { #[inline] fn from_bytes(bytes: [u8; 1]) -> Self { Self::from_ne_bytes(bytes) } }
