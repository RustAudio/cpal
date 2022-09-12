use crate::transcoder;

use super::{Endianness, FromBytes, SampleFormat, ToBytes, BIG_ENDIAN, LITTLE_ENDIAN};

type Sample = i16;
const BYTES: usize = 2;

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

pub struct B2LE {}
transcoder!(
    B2LE,
    Sample,
    BYTES,
    LITTLE_ENDIAN,
    SampleFormat::I16B2(Endianness::Little)
);

pub struct B2BE {}
transcoder!(
    B2BE,
    Sample,
    BYTES,
    BIG_ENDIAN,
    SampleFormat::I16B2(Endianness::Big)
);

#[cfg(target_endian = "big")]
pub type B2NE = B2BE;
#[cfg(target_endian = "little")]
pub type B2NE = B2LE;

// pub enum I16SampleBuffer<'buffer> {
//     B2LE(SampleBuffer<'buffer, B2LE>),
//     B2BE(SampleBuffer<'buffer, B2BE>),
//     B2NE(SampleBuffer<'buffer, B2NE>),
// }

// impl<'buffer> BufferReadAccess<Sample> for I16SampleBuffer<'buffer> {

//     #[inline]
//     fn len(&self) -> usize {
//         match *self {
//             Self::B2LE(ref buffer) => buffer.len(),
//             Self::B2BE(ref buffer) => buffer.len(),
//             Self::B2NE(ref buffer) => buffer.len(),
//         }
//     }

//     #[inline]
//     fn get(&self, index: usize) -> Option<Sample> {
//         match *self {
//             Self::B2LE(ref buffer) => buffer.get(index),
//             Self::B2BE(ref buffer) => buffer.get(index),
//             Self::B2NE(ref buffer) => buffer.get(index),
//         }
//     }

// }

// impl<'buffer> IntoIterator for I16SampleBuffer<'buffer> {
//     type Item = Sample;

//     type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

//     fn into_iter(self) -> Self::IntoIter {
//         match self {
//             I16SampleBuffer::B2LE(ref buffer) => Box::new(buffer.into_iter()),
//             I16SampleBuffer::B2BE(ref buffer) => Box::new(buffer.into_iter()),
//             I16SampleBuffer::B2NE(ref buffer) => Box::new(buffer.into_iter()),
//         }
//     }
// }

// impl<'buffer> IntoIterator for &I16SampleBuffer<'buffer> {
//     type Item = Sample;

//     type IntoIter = Box<dyn Iterator<Item = Sample> + 'buffer>;

//     fn into_iter(self) -> Self::IntoIter {
//         match self {
//             I16SampleBuffer::B2LE(ref buffer) => Box::new(buffer.into_iter()),
//             I16SampleBuffer::B2BE(ref buffer) => Box::new(buffer.into_iter()),
//             I16SampleBuffer::B2NE(ref buffer) => Box::new(buffer.into_iter()),
//         }
//     }
// }

// pub enum I16SampleBufferMut<'buffer> {
//     B2LE(SampleBufferMut<'buffer, B2LE>),
//     B2BE(SampleBufferMut<'buffer, B2BE>),
//     B2NE(SampleBufferMut<'buffer, B2NE>),
// }

// impl<'buffer> BufferReadAccess<Sample> for I16SampleBufferMut<'buffer> {

//     #[inline]
//     fn len(&self) -> usize {
//         match *self {
//             Self::B2LE(ref buffer) => buffer.len(),
//             Self::B2BE(ref buffer) => buffer.len(),
//             Self::B2NE(ref buffer) => buffer.len(),
//         }
//     }

//     #[inline]
//     fn get(&self, index: usize) -> Option<Sample> {
//         match *self {
//             Self::B2LE(ref buffer) => buffer.get(index),
//             Self::B2BE(ref buffer) => buffer.get(index),
//             Self::B2NE(ref buffer) => buffer.get(index),
//         }
//     }

// }

// impl<'buffer> BufferWriteAccess<Sample> for I16SampleBufferMut<'buffer> {

//     #[inline]
//     fn set(&mut self, index: usize, sample: Sample) {
//         match *self {
//             Self::B2LE(ref mut buffer) => buffer.set(index, sample),
//             Self::B2BE(ref mut buffer) => buffer.set(index, sample),
//             Self::B2NE(ref mut buffer) => buffer.set(index, sample),
//         }
//     }

// }
