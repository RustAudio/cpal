use super::{ToBytes, FromBytes, LITTLE_ENDIAN, BIG_ENDIAN};

impl ToBytes<4, LITTLE_ENDIAN> for u32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_le_bytes() } }
impl ToBytes<4, BIG_ENDIAN> for u32 { #[inline] fn to_bytes(self) -> [u8; 4] { self.to_be_bytes() } }
impl FromBytes<4, LITTLE_ENDIAN> for u32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_le_bytes(bytes) } }
impl FromBytes<4, BIG_ENDIAN> for u32 { #[inline] fn from_bytes(bytes: [u8; 4]) -> Self { Self::from_be_bytes(bytes) } }
