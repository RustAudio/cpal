use crate::transcoder;

use super::{Endianness, FromBytes, SampleFormat, ToBytes, BIG_ENDIAN, LITTLE_ENDIAN};

type Sample = f32;
const BYTES: usize = 4;

impl ToBytes<BYTES, LITTLE_ENDIAN> for Sample {
    #[inline]
    fn to_bytes(self) -> [u8; BYTES] {
        self.to_le_bytes()
    }
}
impl ToBytes<BYTES, BIG_ENDIAN> for Sample {
    #[inline]
    fn to_bytes(self) -> [u8; BYTES] {
        self.to_be_bytes()
    }
}
impl FromBytes<BYTES, LITTLE_ENDIAN> for Sample {
    #[inline]
    fn from_bytes(bytes: [u8; BYTES]) -> Self {
        Self::from_le_bytes(bytes)
    }
}
impl FromBytes<BYTES, BIG_ENDIAN> for Sample {
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
    SampleFormat::F32B4(Endianness::Little)
);

pub struct B4BE {}
transcoder!(
    B4BE,
    Sample,
    BYTES,
    BIG_ENDIAN,
    SampleFormat::F32B4(Endianness::Big)
);

#[cfg(target_endian = "big")]
pub type B4NE = B4BE;
#[cfg(target_endian = "little")]
pub type B4NE = B4LE;

// pub enum F32SampleBuffer<'buffer> {
//     B4LE(SampleBuffer<'buffer, B4LE>),
//     B4BE(SampleBuffer<'buffer, B4BE>),
//     B4NE(SampleBuffer<'buffer, B4NE>),
// }

// impl<'buffer> BufferReadAccess<Sample> for F32SampleBuffer<'buffer> {

//     #[inline]
//     fn len(&self) -> usize {
//         match *self {
//             Self::B4LE(ref buffer) => buffer.len(),
//             Self::B4BE(ref buffer) => buffer.len(),
//             Self::B4NE(ref buffer) => buffer.len(),
//         }
//     }

//     #[inline]
//     fn get(&self, index: usize) -> Option<Sample> {
//         match *self {
//             Self::B4LE(ref buffer) => buffer.get(index),
//             Self::B4BE(ref buffer) => buffer.get(index),
//             Self::B4NE(ref buffer) => buffer.get(index),
//         }
//     }

// }

// impl<'buffer> IntoIterator for F32SampleBuffer<'buffer> {
//     type Item = Sample;

//     type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

//     fn into_iter(self) -> Self::IntoIter {
//         match self {
//             F32SampleBuffer::B4LE(ref buffer) => Box::new(buffer.into_iter()),
//             F32SampleBuffer::B4BE(ref buffer) => Box::new(buffer.into_iter()),
//             F32SampleBuffer::B4NE(ref buffer) => Box::new(buffer.into_iter()),
//         }
//     }
// }

// impl<'buffer> IntoIterator for &F32SampleBuffer<'buffer> {
//     type Item = Sample;

//     type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

//     fn into_iter(self) -> Self::IntoIter {
//         match self {
//             F32SampleBuffer::B4LE(ref buffer) => Box::new(buffer.into_iter()),
//             F32SampleBuffer::B4BE(ref buffer) => Box::new(buffer.into_iter()),
//             F32SampleBuffer::B4NE(ref buffer) => Box::new(buffer.into_iter()),
//         }
//     }
// }

// pub enum F32SampleBufferMut<'buffer> {
//     B4LE(SampleBufferMut<'buffer, B4LE>),
//     B4BE(SampleBufferMut<'buffer, B4BE>),
//     B4NE(SampleBufferMut<'buffer, B4NE>),
// }

// impl<'buffer> BufferReadAccess<Sample> for F32SampleBufferMut<'buffer> {

//     #[inline]
//     fn len(&self) -> usize {
//         match *self {
//             Self::B4LE(ref buffer) => buffer.len(),
//             Self::B4BE(ref buffer) => buffer.len(),
//             Self::B4NE(ref buffer) => buffer.len(),
//         }
//     }

//     #[inline]
//     fn get(&self, index: usize) -> Option<Sample> {
//         match *self {
//             Self::B4LE(ref buffer) => buffer.get(index),
//             Self::B4BE(ref buffer) => buffer.get(index),
//             Self::B4NE(ref buffer) => buffer.get(index),
//         }
//     }

// }

// impl<'buffer> BufferWriteAccess<Sample> for F32SampleBufferMut<'buffer> {

//     #[inline]
//     fn set(&mut self, index: usize, sample: Sample) {
//         match *self {
//             Self::B4LE(ref mut buffer) => buffer.set(index, sample),
//             Self::B4BE(ref mut buffer) => buffer.set(index, sample),
//             Self::B4NE(ref mut buffer) => buffer.set(index, sample),
//         }
//     }

// }
