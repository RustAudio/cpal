use Format;
use CreationError;
use StreamData;
use std::marker::PhantomData;
use super::Device;

pub struct EventLoop;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);


pub struct InputBuffer<'a, T: 'a>{
    marker: PhantomData<&'a T>,
}
pub struct OutputBuffer<'a, T: 'a>{
    marker: PhantomData<&'a T>,
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
