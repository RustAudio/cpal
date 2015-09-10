#![allow(dead_code)]

use std::marker::PhantomData;

use CreationError;
use Format;
use FormatsEnumerationError;

#[derive(Default)]
pub struct EndpointsIterator;

impl Iterator for EndpointsIterator {
    type Item = Endpoint;

    fn next(&mut self) -> Option<Endpoint> {
        None
    }
}

pub fn get_default_endpoint() -> Option<Endpoint> {
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        unreachable!()
    }
}

pub struct SupportedFormatsIterator;

impl Iterator for SupportedFormatsIterator {
    type Item = Format;

    fn next(&mut self) -> Option<Format> {
        None
    }
}

pub struct Voice;

impl Voice {
    pub fn new(_: &Endpoint, _: &Format) -> Result<Voice, CreationError> {
        Err(CreationError::DeviceNotAvailable)
    }

    pub fn get_channels(&self) -> ::ChannelsCount {
        unreachable!()
    }

    pub fn get_samples_rate(&self) -> ::SamplesRate {
        unreachable!()
    }

    pub fn get_samples_format(&self) -> ::SampleFormat {
        unreachable!()
    }

    pub fn append_data<'a, T>(&'a mut self, _: usize) -> Buffer<'a, T> {
        unreachable!()
    }

    pub fn play(&mut self) {
    }

    pub fn pause(&mut self) {
    }

    pub fn underflowed(&self) -> bool {
        false
    }
}

pub struct Buffer<'a, T: 'a> {
    marker: PhantomData<&'a T>,
}

impl<'a, T> Buffer<'a, T> {
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        unreachable!()
    }

    pub fn finish(self) {
    }
}
