use os::windows::{Backend, which_backend};

use CreationError;
use DefaultFormatError;
use FormatsEnumerationError;

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
    Asio,
}

pub enum OutputBuffer<'a, T: 'a>{
    Wasapi(wasapi::OutputBuffer<'a, T>),
    Asio,
}

pub enum Devices{
    Wasapi(wasapi::Devices),
    Asio(asio::Devices),
}

pub enum SupportedInputFormats{
    Wasapi(wasapi::SupportedInputFormats),
    Asio(asio::SupportedInputFormats),
}

pub enum SupportedOutputFormats{
    Wasapi(wasapi::SupportedOutputFormats),
    Asio(asio::SupportedOutputFormats),
}

pub fn default_input_device() -> Option<Device> {
    match which_backend() {
        Backend::Wasapi => wasapi::default_input_device(),
        Backend::Asio => None,
    }
}

pub fn default_output_device() -> Option<Device> {
    match which_backend() {
        Backend::Wasapi => wasapi::default_output_device(),
        Backend::Asio => None,
    }
}

impl Default for Devices {
    fn default() -> Devices {
        match which_backend() {
            Backend::Wasapi => wasapi::Devices::default(),
            Backend::Asio => asio::Devices::default(),
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
            Backend::Wasapi => wasapi::EventLoop::new(),
            Backend::Asio => asio::EventLoop::new(),
        }
    }
    
    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        match self {
            &EventLoop::Wasapi(ref d) => d.build_input_stream(device, format),
            &EventLoop::Asio(ref d) => d.build_input_stream(device, format),
        }
    }
    
    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        match self {
            &EventLoop::Wasapi(ref d) => d.build_output_stream(device, format),
            &EventLoop::Asio(ref d) => d.build_output_stream(device, format),
        }
    }
    
    pub fn play_stream(&self, stream: StreamId) {
        match self {
            &EventLoop::Wasapi(ref d) => d.play_stream(stream),
            &EventLoop::Asio(ref d) => d.play_stream(stream),
        }
    }
}
