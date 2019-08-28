extern crate winapi;

use BackendSpecificError;
use BuildStreamError;
use DefaultFormatError;
use DeviceNameError;
use DevicesError;
use Format;
use PlayStreamError;
use PauseStreamError;
use StreamDataResult;
use SupportedFormatsError;
use self::winapi::um::winnt::HRESULT;
use std::io::Error as IoError;
use traits::{EventLoopTrait, HostTrait, StreamIdTrait};
pub use self::device::{Device, Devices, SupportedInputFormats, SupportedOutputFormats, default_input_device, default_output_device};
pub use self::stream::{EventLoop, StreamId};

mod com;
mod device;
mod stream;

/// The WASAPI host, the default windows host type.
///
/// Note: If you use a WASAPI output device as an input device it will
/// transparently enable loopback mode (see
/// https://docs.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording).
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;
    type EventLoop = EventLoop;

    fn is_available() -> bool {
        // Assume WASAPI is always available on windows.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }

    fn event_loop(&self) -> Self::EventLoop {
        EventLoop::new()
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

#[inline]
fn check_result(result: HRESULT) -> Result<(), IoError> {
    if result < 0 {
        Err(IoError::from_raw_os_error(result))
    } else {
        Ok(())
    }
}

fn check_result_backend_specific(result: HRESULT) -> Result<(), BackendSpecificError> {
    match check_result(result) {
        Ok(()) => Ok(()),
        Err(err) => {
            let description = format!("{}", err);
            return Err(BackendSpecificError { description });
        }
    }
}
