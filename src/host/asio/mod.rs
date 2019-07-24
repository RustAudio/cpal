extern crate asio_sys as sys;

use traits::{DeviceTrait, EventLoopTrait, HostTrait, StreamIdTrait};
use {
    BuildStreamError, DefaultFormatError, DeviceNameError, DevicesError, Format, PauseStreamError,
    PlayStreamError, StreamDataResult, SupportedFormatsError,
};

pub use self::device::{Device, Devices, SupportedInputFormats, SupportedOutputFormats};
pub use self::stream::{EventLoop, StreamId};
use std::sync::Arc;

mod device;
mod stream;

/// The host for ASIO.
#[derive(Debug)]
pub struct Host {
    asio: Arc<sys::Asio>,
}

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        let asio = Arc::new(sys::Asio::new());
        let host = Host { asio };
        Ok(host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;
    type EventLoop = EventLoop;

    fn is_available() -> bool {
        true
        //unimplemented!("check how to do this using asio-sys")
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new(self.asio.clone())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        // ASIO has no concept of a default device, so just use the first.
        self.input_devices().ok().and_then(|mut ds| ds.next())
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        // ASIO has no concept of a default device, so just use the first.
        self.output_devices().ok().and_then(|mut ds| ds.next())
    }

    fn event_loop(&self) -> Self::EventLoop {
        EventLoop::new()
    }
}

impl DeviceTrait for Device {
    type SupportedInputFormats = SupportedInputFormats;
    type SupportedOutputFormats = SupportedOutputFormats;

    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    fn supported_input_formats(
        &self,
    ) -> Result<Self::SupportedInputFormats, SupportedFormatsError> {
        Device::supported_input_formats(self)
    }

    fn supported_output_formats(
        &self,
    ) -> Result<Self::SupportedOutputFormats, SupportedFormatsError> {
        Device::supported_output_formats(self)
    }

    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_input_format(self)
    }

    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_output_format(self)
    }
}

impl EventLoopTrait for EventLoop {
    type Device = Device;
    type StreamId = StreamId;

    fn build_input_stream(
        &self,
        device: &Self::Device,
        format: &Format,
    ) -> Result<Self::StreamId, BuildStreamError> {
        EventLoop::build_input_stream(self, device, format)
    }

    fn build_output_stream(
        &self,
        device: &Self::Device,
        format: &Format,
    ) -> Result<Self::StreamId, BuildStreamError> {
        EventLoop::build_output_stream(self, device, format)
    }

    fn play_stream(&self, stream: Self::StreamId) -> Result<(), PlayStreamError> {
        EventLoop::play_stream(self, stream)
    }

    fn pause_stream(&self, stream: Self::StreamId) -> Result<(), PauseStreamError> {
        EventLoop::pause_stream(self, stream)
    }

    fn destroy_stream(&self, stream: Self::StreamId) {
        EventLoop::destroy_stream(self, stream)
    }

    fn run<F>(&self, callback: F) -> !
    where
        F: FnMut(Self::StreamId, StreamDataResult) + Send,
    {
        EventLoop::run(self, callback)
    }
}

impl StreamIdTrait for StreamId {}
