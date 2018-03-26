use os::windows::{Backend, which_backend};

#[cfg(windows)]
mod asio;

#[cfg(windows)]
mod wasapi;

pub enum EventLoop {
    Wasapi(wasapi::EventLoop),
    Asio,
}

pub enum Device {
    Wasapi(wasapi::Device),
    Asio,
}

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
    Asio,
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
