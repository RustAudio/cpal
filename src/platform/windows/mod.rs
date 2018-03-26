use os::windows::{Backend, which_backend};

#[cfg(windows)]
mod asio;

#[cfg(windows)]
mod wasapi;

pub enum EventLoop {
    Wasapi(wasapi::EventLoop),
    Asio,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Device {
    Wasapi(wasapi::Device),
    Asio(asio::Device),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamId{
    Wasapi(wasapi::StreamId),
    Asio
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
    Asio,
}

pub enum SupportedOutputFormats{
    Wasapi(wasapi::SupportedOutputFormats),
    Asio,
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
}
