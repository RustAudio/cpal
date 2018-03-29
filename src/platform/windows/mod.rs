use std;
use os::windows::{Backend, which_backend};

use CreationError;
use DefaultFormatError;
use FormatsEnumerationError;
use SupportedFormat;
use StreamData;

use Format;

#[cfg(windows)]
mod asio;

#[cfg(windows)]
mod wasapi;

pub enum EventLoop {
    Wasapi(wasapi::EventLoop),
    Asio(asio::EventLoop),
}

#[derive(Clone, PartialEq, Eq)]
pub enum Device {
    Wasapi(wasapi::Device),
    Asio(asio::Device),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamId{
    Wasapi(wasapi::StreamId),
    Asio(asio::StreamId),
}

pub enum InputBuffer<'a, T: 'a>{
    Wasapi(wasapi::InputBuffer<'a, T>),
    Asio(asio::InputBuffer<'a, T>),
}

pub enum OutputBuffer<'a, T: 'a>{
    Wasapi(wasapi::OutputBuffer<'a, T>),
    Asio(asio::OutputBuffer<'a, T>),
}

/*
pub enum UnknownTypeInputBuffer<'a> {
    /// Samples whose format is `u16`.
    U16(InputBuffer<'a, u16>),
    /// Samples whose format is `i16`.
    I16(InputBuffer<'a, i16>),
    /// Samples whose format is `f32`.
    F32(InputBuffer<'a, f32>),
}

pub enum UnknownTypeOutputBuffer<'a> {
    /// Samples whose format is `u16`.
    U16(OutputBuffer<'a, u16>),
    /// Samples whose format is `i16`.
    I16(OutputBuffer<'a, i16>),
    /// Samples whose format is `f32`.
    F32(OutputBuffer<'a, f32>),
}

pub enum StreamData<'a> {
    Input {
        buffer: UnknownTypeInputBuffer<'a>,
    },
    Output {
        buffer: UnknownTypeOutputBuffer<'a>,
    },
}
*/

pub enum Devices{
    Wasapi(wasapi::Devices),
    Asio(asio::Devices),
}

pub type SupportedInputFormats = std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = std::vec::IntoIter<SupportedFormat>;

pub fn default_input_device() -> Option<Device> {
    match which_backend() {
        &Backend::Wasapi => {
            match wasapi::default_input_device(){
                Some(d) => Some( Device::Wasapi(d) ),
                None => None
            }
        },
        &Backend::Asio => None,
    }
}

pub fn default_output_device() -> Option<Device> {
    match which_backend() {
        &Backend::Wasapi => {
            match wasapi::default_output_device(){
                Some(d) => Some( Device::Wasapi(d) ),
                None => None
            }
        },
        &Backend::Asio => None,
    }
}

impl Default for Devices {
    fn default() -> Devices {
        match which_backend() {
            &Backend::Wasapi => Devices::Wasapi( wasapi::Devices::default() ),
            &Backend::Asio => Devices::Asio( asio::Devices::default() ),
        }
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        match self {
            &mut Devices::Wasapi(ref mut d) => {
                match d.next(){
                    Some(n) => Some(Device::Wasapi(n)),
                    None => None,
                }
            },
            &mut Devices::Asio(ref mut d) => {
                match d.next(){
                    Some(n) => Some(Device::Asio(n)),
                    None => None,
                }
            },
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            &Devices::Wasapi(ref d) => d.size_hint(),
            &Devices::Asio(ref d) => d.size_hint(),
        }
    }
}

impl Device {
    pub fn name(&self) -> String {
        match self {
            &Device::Wasapi(ref d) => d.name(),
            &Device::Asio(ref d) => d.name(),
        }
    }
    
    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, 
    FormatsEnumerationError> {
        match self {
            &Device::Wasapi(ref d) => d.supported_input_formats(),
            &Device::Asio(ref d) => d.supported_input_formats(),
        }
    }
    
    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, 
    FormatsEnumerationError> {
        match self {
            &Device::Wasapi(ref d) => d.supported_output_formats(),
            &Device::Asio(ref d) => d.supported_output_formats(),
        }
    }

    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        match self {
            &Device::Wasapi(ref d) => d.default_input_format(),
            &Device::Asio(ref d) => d.default_input_format(),
        }
    }
    
    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        match self {
            &Device::Wasapi(ref d) => d.default_output_format(),
            &Device::Asio(ref d) => d.default_output_format(),
        }
    }
}


impl EventLoop {
    pub fn new() -> EventLoop {
        match which_backend() {
            &Backend::Wasapi => EventLoop::Wasapi( wasapi::EventLoop::new() ),
            &Backend::Asio => EventLoop::Asio( asio::EventLoop::new() ),
        }
    }
    
    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        match (self, device) {
            (&EventLoop::Wasapi(ref ev), &Device::Wasapi(ref dev)) => ev.build_input_stream(dev, format).map(|id| StreamId::Wasapi(id)),
            (&EventLoop::Asio(ref ev), &Device::Asio(ref dev)) => ev.build_input_stream(dev, format).map(|id| StreamId::Asio(id)),
            _ => unreachable!(),
        }
    }
    
    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        match (self, device) {
            (&EventLoop::Wasapi(ref ev), &Device::Wasapi(ref dev)) => ev.build_output_stream(dev, format).map(|id| StreamId::Wasapi(id)),
            (&EventLoop::Asio(ref ev), &Device::Asio(ref dev)) => ev.build_output_stream(dev, format).map(|id| StreamId::Asio(id)),
            _ => unreachable!(),
        }
    }
    
    pub fn play_stream(&self, stream: StreamId) {
        match self {
            &EventLoop::Wasapi(ref d) => match stream {
                StreamId::Wasapi(s) => d.play_stream(s),
                _ => unreachable!(),
            },
            &EventLoop::Asio(ref d) => match stream {
                StreamId::Asio(s) => d.play_stream(s),
                _ => unreachable!(),
            },
        }
    }
    pub fn pause_stream(&self, stream: StreamId) {
        match self {
            &EventLoop::Wasapi(ref d) => match stream {
                StreamId::Wasapi(s) => d.pause_stream(s),
                _ => unreachable!(),
            },
            &EventLoop::Asio(ref d) => match stream {
                StreamId::Asio(s) => d.pause_stream(s),
                _ => unreachable!(),
            },
        }
    }
    pub fn destroy_stream(&self, stream: StreamId) {
        match self {
            &EventLoop::Wasapi(ref d) => match stream {
                StreamId::Wasapi(s) => d.destroy_stream(s),
                _ => unreachable!(),
            },
            &EventLoop::Asio(ref d) => match stream {
                StreamId::Asio(s) => d.destroy_stream(s),
                _ => unreachable!(),
            },
        }
    }
    
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData)
        {
            match self {
                &EventLoop::Wasapi(ref d) => {
                    d.run( |id, data| callback(StreamId::Wasapi(id), data) )
                },
                &EventLoop::Asio(ref d) => {
                    d.run( |id, data| callback(StreamId::Asio(id), data) )
                },
            }
        }
}


impl<'a, T> InputBuffer<'a, T> {
    pub fn buffer(&self) -> &[T] {
        match self {
            &InputBuffer::Wasapi(ref d) => d.buffer(),
            &InputBuffer::Asio(ref d) => d.buffer(),
        }
    }
    pub fn finish(self) {
        match self {
            InputBuffer::Wasapi(d) => d.finish(),
            InputBuffer::Asio(d) => d.finish(),
        }
    }
}

impl<'a, T> OutputBuffer<'a, T> {
    pub fn buffer(&mut self) -> &mut [T] {
        match self {
            &mut OutputBuffer::Wasapi(ref mut d) => d.buffer(),
            &mut OutputBuffer::Asio(ref mut d) => d.buffer(),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            &OutputBuffer::Wasapi(ref d) => d.len(),
            &OutputBuffer::Asio(ref d) => d.len(),
        }
    }

    pub fn finish(self) {
        match self {
            OutputBuffer::Wasapi(d) => d.finish(),
            OutputBuffer::Asio(d) => d.finish(),
        }
    }
}

/*
fn convert_stream_type(data: ::StreamData) -> StreamData {
    match data {
        ::StreamData::Input(b) => {
            match b {
                ::UnknownTypeInputBuffer::U16(ob) => ob,
            }
        },
        ::StreamData::Output(b) => b,
    }
    match un_buf {
        ::Un
    }
}
*/
