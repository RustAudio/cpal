extern crate asio_sys as sys;

use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat,
    StreamConfig, StreamError, SupportedStreamConfig, SupportedStreamConfigsError,
};

pub use self::device::{Device, Devices, SupportedInputConfigs, SupportedOutputConfigs};
pub use self::stream::Stream;
use std::sync::Arc;
use std::time::Duration;

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
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Device::supported_input_configs(self)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Device::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
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
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
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
    fn play(&self) -> Result<(), PlayStreamError> {
        Stream::play(self)
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        Stream::pause(self)
    }
}
