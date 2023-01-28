extern crate pipewire;

use std::rc::Rc;
use std::sync::mpsc;

use crate::traits::HostTrait;
use crate::{DevicesError, SampleFormat, SupportedStreamConfigRange};

mod device;
pub use self::device::Device;
pub use self::stream::Stream;
mod conn;
mod stream;

const PIPEWIRE_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type Devices = std::vec::IntoIter<Device>;

/// The PipeWire Host type

pub struct Host {
    /// The name that the client will have in PipeWire.
    /// Until we have duplex streams two clients will be created adding "out" or "in" to the name
    /// since names have to be unique.
    name: String,
    /// If ports are to be connected to the system (soundcard) ports automatically (default is true).
    connect_ports_automatically: bool,
    /// A list of the devices that have been created from this Host.
    devices_created: Vec<Device>,

    client: Rc<conn::PWClient>,
}

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        let client = Rc::new(conn::PWClient::new());

        let mut host = Host {
            name: "cpal_client".to_owned(),
            connect_ports_automatically: true,
            devices_created: vec![],
            client,
        };

        // Devices don't exist for PipeWire, they have to be created
        host.initialize_default_devices();
        Ok(host)
    }
    /// Set whether the ports should automatically be connected to system
    /// (default is true)
    pub fn set_connect_automatically(&mut self, do_connect: bool) {
        self.connect_ports_automatically = do_connect;
    }

    pub fn input_device_with_name(&mut self, name: &str) -> Option<Device> {
        self.name = name.to_owned();
        self.default_input_device()
    }

    pub fn output_device_with_name(&mut self, name: &str) -> Option<Device> {
        self.name = name.to_owned();
        self.default_output_device()
    }

    fn initialize_default_devices(&mut self) {
        let in_device_res = Device::default_input_device(
            &self.name,
            self.connect_ports_automatically,
            self.client.clone(),
        );

        match in_device_res {
            Ok(device) => self.devices_created.push(device),
            Err(err) => {
                println!("{}", err);
            }
        }

        let out_device_res = Device::default_output_device(
            &self.name,
            self.connect_ports_automatically,
            self.client.clone(),
        );
        match out_device_res {
            Ok(device) => self.devices_created.push(device),
            Err(err) => {
                println!("{}", err);
            }
        }
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Ok(self.devices_created.clone().into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        for device in &self.devices_created {
            if device.is_input() {
                return Some(device.clone());
            }
        }
        None
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        for device in &self.devices_created {
            if device.is_output() {
                return Some(device.clone());
            }
        }
        None
    }
}
