#![allow(dead_code)]

use BuildStreamError;
use DefaultFormatError;
use DevicesError;
use DeviceNameError;
use Format;
use PauseStreamError;
use PlayStreamError;
use StreamDataResult;
use SupportedFormatsError;
use SupportedFormat;
use traits::{DeviceTrait, EventLoopTrait, HostTrait, StreamIdTrait};

#[derive(Default)]
pub struct Devices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct EventLoop;

pub struct Host;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId;

pub struct SupportedInputFormats;
pub struct SupportedOutputFormats;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices)
    }
}

impl EventLoop {
    pub fn new() -> EventLoop {
        EventLoop
    }
}

impl DeviceTrait for Device {
    type SupportedInputFormats = SupportedInputFormats;
    type SupportedOutputFormats = SupportedOutputFormats;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("null".to_owned())
    }

    #[inline]
    fn supported_input_formats(&self) -> Result<SupportedInputFormats, SupportedFormatsError> {
        unimplemented!()
    }

    #[inline]
    fn supported_output_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        unimplemented!()
    }

    #[inline]
    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!()
    }

    #[inline]
    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!()
    }
}

impl EventLoopTrait for EventLoop {
    type Device = Device;
    type StreamId = StreamId;

    #[inline]
    fn run<F>(&self, _callback: F) -> !
        where F: FnMut(StreamId, StreamDataResult)
    {
        loop { /* TODO: don't spin */ }
    }

    #[inline]
    fn build_input_stream(&self, _: &Device, _: &Format) -> Result<StreamId, BuildStreamError> {
        Err(BuildStreamError::DeviceNotAvailable)
    }

    #[inline]
    fn build_output_stream(&self, _: &Device, _: &Format) -> Result<StreamId, BuildStreamError> {
        Err(BuildStreamError::DeviceNotAvailable)
    }

    #[inline]
    fn destroy_stream(&self, _: StreamId) {
        unimplemented!()
    }

    #[inline]
    fn play_stream(&self, _: StreamId) -> Result<(), PlayStreamError> {
        panic!()
    }

    #[inline]
    fn pause_stream(&self, _: StreamId) -> Result<(), PauseStreamError> {
        panic!()
    }
}

impl HostTrait for Host {
    type Device = Device;
    type Devices = Devices;
    type EventLoop = EventLoop;

    fn is_available() -> bool {
        false
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Device> {
        None
    }

    fn default_output_device(&self) -> Option<Device> {
        None
    }

    fn event_loop(&self) -> Self::EventLoop {
        EventLoop::new()
    }
}

impl StreamIdTrait for StreamId {}

impl Iterator for Devices {
    type Item = Device;

    #[inline]
    fn next(&mut self) -> Option<Device> {
        None
    }
}

impl Iterator for SupportedInputFormats {
    type Item = SupportedFormat;

    #[inline]
    fn next(&mut self) -> Option<SupportedFormat> {
        None
    }
}

impl Iterator for SupportedOutputFormats {
    type Item = SupportedFormat;

    #[inline]
    fn next(&mut self) -> Option<SupportedFormat> {
        None
    }
}
