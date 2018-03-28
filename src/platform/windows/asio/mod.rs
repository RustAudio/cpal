use std;
extern crate asio_sys as sys;

use CreationError;
use DefaultFormatError;
use FormatsEnumerationError;
use StreamData;
use std::marker::PhantomData;
use SupportedFormat;

use Format;

pub struct Devices{
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Device;

pub struct EventLoop;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

pub type SupportedInputFormats = std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = std::vec::IntoIter<SupportedFormat>;

pub struct InputBuffer<'a, T: 'a>{
    marker: PhantomData<&'a T>,
}
pub struct OutputBuffer<'a, T: 'a>{
    marker: PhantomData<&'a T>,
}
impl Default for Devices {
    fn default() -> Devices {
        Devices{}
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        unimplemented!()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        unimplemented!()
    }
}

impl Device {
    pub fn name(&self) -> String {
        "".to_owned()
    }
    
    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, 
    FormatsEnumerationError> {
        unimplemented!()
    }
    
    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, 
    FormatsEnumerationError> {
        unimplemented!()
    }
    
    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!()
    }
    
    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!()
    }
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop
    }

    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        unimplemented!()
    }

    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        unimplemented!()
    }
    
    pub fn play_stream(&self, stream: StreamId) {
        unimplemented!()
    }
    
    pub fn pause_stream(&self, stream: StreamId) {
        unimplemented!()
    }
    pub fn destroy_stream(&self, stream_id: StreamId) {
        unimplemented!()
    }
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData)
        {
            unimplemented!()
        }
}

impl<'a, T> InputBuffer<'a, T> {
    pub fn buffer(&self) -> &[T] {
        unimplemented!()
    }
    pub fn finish(self) {
        unimplemented!()
    }
}

impl<'a, T> OutputBuffer<'a, T> {
    pub fn buffer(&mut self) -> &mut [T] {
        unimplemented!()
    }

    pub fn len(&self) -> usize {
        unimplemented!()
    }

    pub fn finish(self) {
        unimplemented!()
    }
}
