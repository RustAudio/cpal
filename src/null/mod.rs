#![allow(dead_code)]

use std::marker::PhantomData;

use CreationError;
use Format;
use FormatsEnumerationError;
use SupportedFormat;
use UnknownTypeBuffer;

pub struct EventLoop;
impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        EventLoop
    }

    #[inline]
    pub fn run<F>(&self, _callback: F) -> !
        where F: FnMut(VoiceId, UnknownTypeBuffer)
    {
        loop { /* TODO: don't spin */ }
    }

    #[inline]
    pub fn build_voice(&self, _: &Endpoint, _: &Format)
                       -> Result<VoiceId, CreationError>
    {
        Err(CreationError::DeviceNotAvailable)
    }

    #[inline]
    pub fn destroy_voice(&self, _: VoiceId) {
        unreachable!()
    }

    #[inline]
    pub fn play(&self, _: VoiceId) {
        panic!()
    }

    #[inline]
    pub fn pause(&self, _: VoiceId) {
        panic!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoiceId;

#[derive(Default)]
pub struct EndpointsIterator;

impl Iterator for EndpointsIterator {
    type Item = Endpoint;

    #[inline]
    fn next(&mut self) -> Option<Endpoint> {
        None
    }
}

#[inline]
pub fn default_endpoint() -> Option<Endpoint> {
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    #[inline]
    pub fn supported_formats(
        &self)
        -> Result<SupportedFormatsIterator, FormatsEnumerationError> {
        unreachable!()
    }

    #[inline]
    pub fn name(&self) -> String {
        "null".to_owned()
    }
}

pub struct SupportedFormatsIterator;

impl Iterator for SupportedFormatsIterator {
    type Item = SupportedFormat;

    #[inline]
    fn next(&mut self) -> Option<SupportedFormat> {
        None
    }
}

pub struct Buffer<'a, T: 'a> {
    marker: PhantomData<&'a mut T>,
}

impl<'a, T> Buffer<'a, T> {
    #[inline]
    pub fn buffer(&mut self) -> &mut [T] {
        unreachable!()
    }

    #[inline]
    pub fn len(&self) -> usize {
        0
    }

    #[inline]
    pub fn finish(self) {
    }
}
