#![allow(dead_code)]

use std::marker::PhantomData;

use futures::Poll;
use futures::stream::Stream;
use futures::Async;

use CreationError;
use Format;
use FormatsEnumerationError;
use UnknownTypeBuffer;

pub struct EventLoop;
impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop { EventLoop }
    #[inline]
    pub fn run(&self) { loop { /* TODO: don't spin */ } }
}

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
pub fn get_default_endpoint() -> Option<Endpoint> {
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    #[inline]
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        unreachable!()
    }

    #[inline]
    pub fn get_name(&self) -> String {
        "null".to_owned()
    }
}

pub struct SupportedFormatsIterator;

impl Iterator for SupportedFormatsIterator {
    type Item = Format;

    #[inline]
    fn next(&mut self) -> Option<Format> {
        None
    }
}

pub struct Voice;
pub struct SamplesStream;

impl Voice {
    #[inline]
    pub fn new(_: &Endpoint, _: &Format, _: &EventLoop)
               -> Result<(Voice, SamplesStream), CreationError>
    {
        Err(CreationError::DeviceNotAvailable)
    }

    #[inline]
    pub fn play(&mut self) {
    }

    #[inline]
    pub fn pause(&mut self) {
    }
}

impl Stream for SamplesStream {
    type Item = UnknownTypeBuffer;
    type Error = ();

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        Ok(Async::NotReady)
    }
}

pub struct Buffer<T> {
    marker: PhantomData<T>,
}

impl<T> Buffer<T> {
    #[inline]
    pub fn get_buffer(&mut self) -> &mut [T] {
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
