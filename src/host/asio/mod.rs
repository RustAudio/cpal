//! ASIO backend implementation.
//!
//! ASIO is available on Windows with the `asio` feature.
//! See the project README for setup instructions.

extern crate asio_sys as sys;

use crate::host::com;
use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    Data, DeviceDescription, DeviceId, Error, InputCallbackInfo, OutputCallbackInfo, SampleFormat,
    StreamConfig, SupportedStreamConfig,
};

pub use self::device::{Device, Devices, SupportedInputConfigs, SupportedOutputConfigs};
pub use self::stream::Stream;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

mod device;
mod stream;

/// Global ASIO instance shared across all Host instances.
///
/// ASIO only supports loading a single driver at a time globally, so all Host instances
/// must share the same underlying sys::Asio wrapper to properly coordinate driver access.
static GLOBAL_ASIO: OnceLock<Arc<sys::Asio>> = OnceLock::new();

/// The host for ASIO.
#[derive(Debug)]
pub struct Host {
    asio: Arc<sys::Asio>,
}

impl Host {
    pub fn new() -> Result<Self, crate::Error> {
        com::com_initialized();
        let asio = GLOBAL_ASIO
            .get_or_init(|| Arc::new(sys::Asio::new()))
            .clone();
        let host = Host { asio };
        Ok(host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        true
        //unimplemented!("check how to do this using asio-sys")
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
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
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        Device::description(self)
    }

    fn id(&self) -> Result<DeviceId, Error> {
        Device::id(self)
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        Device::supported_input_configs(self)
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        Device::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        Device::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        Device::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        Device::build_input_stream_raw(
            self,
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        Device::build_output_stream_raw(
            self,
            config,
            sample_format,
            data_callback,
            error_callback,
            timeout,
        )
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        Stream::play(self)
    }

    fn pause(&self) -> Result<(), Error> {
        Stream::pause(self)
    }

    fn now(&self) -> crate::StreamInstant {
        Stream::now(self)
    }

    fn buffer_size(&self) -> Result<crate::FrameCount, Error> {
        Stream::buffer_size(self)
    }
}
