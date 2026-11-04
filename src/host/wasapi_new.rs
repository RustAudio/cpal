use crate::traits::*;
use crate::*;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::time::Duration;
use std::vec;

type SupportedConfigs = vec::IntoIter<SupportedStreamConfigRange>;

pub struct Host;

impl Host {
    #[expect(
        clippy::unnecessary_wraps,
        reason = "required by the `impl_platform_host` macro"
    )]
    pub const fn new() -> Result<Self, Error> {
        Ok(Self)
    }
}

impl HostTrait for Host {
    type Devices = vec::IntoIter<Device>;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        todo!()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        todo!()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        todo!()
    }
}

impl Display for Device {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedConfigs;
    type SupportedOutputConfigs = SupportedConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, Error> {
        todo!()
    }

    fn id(&self) -> Result<DeviceId, Error> {
        todo!()
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        todo!()
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        todo!()
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        todo!()
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        todo!()
    }

    fn build_input_stream_raw<DataCallback, ErrorCallback>(
        &self,
        config  : StreamConfig,
        format  : SampleFormat,
        data_cb : DataCallback,
        error_cb: ErrorCallback,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        DataCallback : FnMut(&Data, &CallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(Error) + Send + 'static
    {
        todo!()
    }

    fn build_output_stream_raw<DataCallback, ErrorCallback>(
        &self,
        config: StreamConfig,
        format: SampleFormat,
        data_cb: DataCallback,
        error_cb: ErrorCallback,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, Error>
    where
        DataCallback: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(Error) + Send + 'static,
    {
        todo!()
    }
}

pub struct Stream;

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        todo!()
    }

    fn pause(&self) -> Result<(), Error> {
        todo!()
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        todo!()
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        todo!()
    }

    fn now(&self) -> StreamInstant {
        todo!()
    }
}
