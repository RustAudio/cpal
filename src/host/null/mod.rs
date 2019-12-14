use BuildStreamError;
use DefaultFormatError;
use DevicesError;
use DeviceNameError;
use Format;
use PauseStreamError;
use PlayStreamError;
use StreamData;
use StreamError;
use SupportedFormatsError;
use SupportedFormat;
use traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Default)]
pub struct Devices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Host;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stream;

pub struct SupportedInputFormats;
pub struct SupportedOutputFormats;

impl Host {
    #[allow(dead_code)]
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        Ok(Devices)
    }
}

impl DeviceTrait for Device {
    type SupportedInputFormats = SupportedInputFormats;
    type SupportedOutputFormats = SupportedOutputFormats;
    type Stream = Stream;

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

    fn build_input_stream<D, E>(&self, _format: &Format, _data_callback: D, _error_callback: E) -> Result<Self::Stream, BuildStreamError>
        where D: FnMut(StreamData) + Send + 'static, E: FnMut(StreamError) + Send + 'static {
        unimplemented!()
    }

    /// Create an output stream.
    fn build_output_stream<D, E>(&self, _format: &Format, _data_callback: D, _error_callback: E) -> Result<Self::Stream, BuildStreamError>
        where D: FnMut(StreamData) + Send + 'static, E: FnMut(StreamError) + Send + 'static{
        unimplemented!()
    }
}

impl HostTrait for Host {
    type Device = Device;
    type Devices = Devices;

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
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        unimplemented!()
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        unimplemented!()
    }
}

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
