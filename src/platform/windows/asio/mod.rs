extern crate asio_sys as sys;

use CreationError;
use DefaultFormatError;
use FormatsEnumerationError;

use Format;

pub struct Devices{
}

pub struct Device;

pub struct EventLoop;

pub struct SupportedInputFormats;
pub struct SupportedOutputFormats;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

impl Default for Devices {
    fn default() -> Devices {
        Devices{}
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
}
