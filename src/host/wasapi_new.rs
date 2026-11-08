use crate::traits::*;
use crate::*;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::time::Duration;
use std::vec;
use parking_lot::Mutex;
use tap::Pipe;
use windows::Devices::Enumeration::DeviceInformation;

use self::utils::CpalError;

mod device;
mod utils;

type SupportedConfigs = vec::IntoIter<SupportedStreamConfigRange>;

pub struct Host;

impl Host {
    #[expect(
        clippy::unnecessary_wraps,
        reason = "required by the `impl_platform_host` macro"
    )]
    pub const fn new() -> Result<Self, CpalError> {
        Ok(Self)
    }
}

impl HostTrait for Host {
    type Devices = vec::IntoIter<Device>;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, CpalError> {
        device::all()?
            .into_iter()
            .pipe(Ok)
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        device::default::<true>().ok()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        device::default::<false>().ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    info: DeviceInformation,
    direction: DeviceDirection,
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

    fn description(&self) -> Result<DeviceDescription, CpalError> {
        let name = self.info.Name()?.to_string_lossy();
        let reader = device::PropertyReader(self.info.Properties()?);
        let (device_type, interface_type) = reader.types()?;
        let mut builder = DeviceDescriptionBuilder
            ::new(name.clone())
            .driver(name)
            .direction(self.direction)
            .device_type(device_type)
            .interface_type(interface_type);
        if let Some(physical_location) = reader.physical_location()? {
            builder = builder.address(physical_location);
        }
        Ok(builder.build())
    }

    fn id(&self) -> Result<DeviceId, CpalError> {
        self.info
            .Id()?
            .to_string_lossy()
            .into_boxed_str()
            .pipe(|id| DeviceId(HostId::WasapiNew, id))
            .pipe(Ok)
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, CpalError> {
        todo!()
    }

    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, CpalError> {
        todo!()
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, CpalError> {
        todo!()
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, CpalError> {
        todo!()
    }

    fn build_input_stream_raw<DataCallback, ErrorCallback>(
        &self,
        config  : StreamConfig,
        format  : SampleFormat,
        data_cb : DataCallback,
        error_cb: ErrorCallback,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, CpalError>
    where
        DataCallback : FnMut(&Data, &CallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(CpalError) + Send + 'static
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
    ) -> Result<Self::Stream, CpalError>
    where
        DataCallback: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        ErrorCallback: FnMut(CpalError) + Send + 'static,
    {
        todo!()
    }
}

pub struct Stream(Mutex<StreamInner>);

impl From<StreamInner> for Stream {
    fn from(value: StreamInner) -> Self {
        value
            .pipe(Mutex::new)
            .pipe(Self)
    }
}
impl StreamTrait for Stream {
    fn start(&self) -> Result<(), CpalError> {
        self.0.lock().start()
    }

    fn pause(&self) -> Result<(), CpalError> {
        self.0.lock().pause()
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), CpalError> {
        self.0.lock().stop(timeout)
    }

    fn buffer_size(&self) -> Result<FrameCount, CpalError> {
        self.0.lock().buffer_size()
    }

    fn now(&self) -> StreamInstant {
        self.0.lock().now()
    }
}

struct StreamInner {
}

impl StreamInner {
    pub fn start(&mut self) -> Result<(), CpalError> {
        todo!()
    }

    pub fn pause(&mut self) -> Result<(), CpalError> {
        todo!()
    }

    pub fn stop(&mut self, timeout: Option<Duration>) -> Result<(), CpalError> {
        todo!()
    }

    pub fn buffer_size(&self) -> Result<FrameCount, CpalError> {
        todo!()
    }

    pub fn now(&mut self) -> StreamInstant {
        todo!()
    }
}