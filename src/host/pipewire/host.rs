use crate::traits::HostTrait;
use crate::{BackendSpecificError, DevicesError, HostUnavailable, SupportedStreamConfigRange};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use pipewire_client::{Direction, PipewireClient};
use tokio::runtime::Runtime;
use crate::host::pipewire::Device;

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type Devices = std::vec::IntoIter<Device>;

#[derive(Debug)]
pub struct Host {
    runtime: Arc<Runtime>,
    client: Rc<PipewireClient>,
}

impl Host {
    pub fn new() -> Result<Self, HostUnavailable> {
        let timeout = Duration::from_secs(30);
        let runtime = Arc::new(Runtime::new().unwrap());
        let client = PipewireClient::new(runtime.clone(), timeout)
            .map_err(move |error| {
                eprintln!("{}", error.description);
                HostUnavailable
            })?;
        let client = Rc::new(client);
        let host = Host { 
            runtime, 
            client 
        };
        Ok(host)
    }

    fn default_device(&self, direction: Direction) -> Option<Device> {
        self.devices()
            .unwrap()
            .filter(move |device| device.direction == direction && device.is_default)
            .collect::<Vec<_>>()
            .first()
            .cloned()
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        let input_devices = match self.client.node().enumerate(Direction::Input) {
            Ok(values) => values.into_iter(),
            Err(value) => return Err(DevicesError::BackendSpecific {
                err: BackendSpecificError {
                    description: value.description,
                },
            }),
        };
        let output_devices = match self.client.node().enumerate(Direction::Output) {
            Ok(values) => values.into_iter(),
            Err(value) => return Err(DevicesError::BackendSpecific {
                err: BackendSpecificError {
                    description: value.description,
                },
            }),
        };
        let devices = input_devices.chain(output_devices)
            .map(move |device| {
                Device::from(&device, self.client.clone()).unwrap()
            })
            .collect::<Vec<_>>()
            .into_iter();
        Ok(devices)
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.default_device(Direction::Input)
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.default_device(Direction::Output)
    }
}