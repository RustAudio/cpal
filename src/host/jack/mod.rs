extern crate jack;

use crate::{
    BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    PauseStreamError, PlayStreamError, SampleFormat, StreamConfig, StreamError,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use traits::{DeviceTrait, HostTrait, StreamTrait};

use std::cell::RefCell;

mod device;
pub use self::device::Device;
pub use self::stream::Stream;
mod stream;

const JACK_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

pub type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
pub type Devices = std::vec::IntoIter<Device>;

/// The JACK Host type
#[derive(Debug)]
pub struct Host {
    /// The name that the client will have in JACK.
    /// Until we have duplex streams two clients will be created adding "out" or "in" to the name
    /// since names have to be unique.
    name: String,
    /// If ports are to be connected to the system (soundcard) ports automatically (default is true).
    connect_ports_automatically: bool,
    /// If the JACK server should be started automatically if it isn't already when creating a Client (default is false).
    start_server_automatically: bool,
    /// A list of the devices that have been created from this Host.
    devices_created: RefCell<Vec<Device>>,
}

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host {
            name: "cpal_client".to_owned(),
            connect_ports_automatically: true,
            start_server_automatically: false,
            devices_created: RefCell::new(vec![]),
        })
    }
    /// Set whether the ports should automatically be connected to system
    /// (default is true)
    pub fn set_connect_automatically(&mut self, do_connect: bool) {
        self.connect_ports_automatically = do_connect;
    }
    /// Set whether a JACK server should be automatically started if it isn't already.
    /// (default is false)
    pub fn set_start_server_automatically(&mut self, do_start_server: bool) {
        self.start_server_automatically = do_start_server;
    }

    pub fn input_device_with_name(&mut self, name: &str) -> Option<Device> {
        self.name = name.to_owned();
        self.default_input_device()
    }

    pub fn output_device_with_name(&mut self, name: &str) -> Option<Device> {
        self.name = name.to_owned();
        self.default_output_device()
    }
}

impl HostTrait for Host {
    type Device = Device;
    type Devices = Devices;

    fn is_available() -> bool {
        // TODO: Determine if JACK is available. What does that mean? That the server is started?
        // To properly check if the server is started we need to create a Client, but we cannot do this
        // until we know what name to give the client.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Ok(self.devices_created.borrow().clone().into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        // TODO: Check if a device with that name was already created and add it to the list when created if it wasn't
        let device_res = Device::default_input_device(
            &self.name,
            self.connect_ports_automatically,
            self.start_server_automatically,
        );
        match device_res {
            Ok(device) => Some(device),
            Err(err) => {
                println!("{}", err);
                None
            }
        }
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        // TODO: Check if a device with that name was already created and add it to the list when created if it wasn't
        let device_res = Device::default_output_device(
            &self.name,
            self.connect_ports_automatically,
            self.start_server_automatically,
        );
        match device_res {
            Ok(device) => Some(device),
            Err(err) => {
                println!("{}", err);
                None
            }
        }
    }
}

fn get_client_options(start_server_automatically: bool) -> jack::ClientOptions {
    let mut client_options = jack::ClientOptions::empty();
    client_options.set(
        jack::ClientOptions::NO_START_SERVER,
        !start_server_automatically,
    );
    client_options
}

fn get_client(name: &str, client_options: jack::ClientOptions) -> Result<jack::Client, String> {
    let c_res = jack::Client::new(name, client_options);
    match c_res {
        Ok((client, status)) => {
            // The ClientStatus can tell us many things
            println!(
                "Managed to open client {}, with status {:?}!",
                client.name(),
                status
            );
            if status.intersects(jack::ClientStatus::SERVER_ERROR) {
                return Err(String::from(
                    "There was an error communicating with the JACK server!",
                ));
            } else if status.intersects(jack::ClientStatus::SERVER_FAILED) {
                return Err(String::from("Could not connect to the JACK server!"));
            } else if status.intersects(jack::ClientStatus::VERSION_ERROR) {
                return Err(String::from(
                    "Error connecting to JACK server: Client's protocol version does not match!",
                ));
            } else if status.intersects(jack::ClientStatus::INIT_FAILURE) {
                return Err(String::from(
                    "Error connecting to JACK server: Unable to initialize client!",
                ));
            } else if status.intersects(jack::ClientStatus::SHM_FAILURE) {
                return Err(String::from(
                    "Error connecting to JACK server: Unable to access shared memory!",
                ));
            } else if status.intersects(jack::ClientStatus::NO_SUCH_CLIENT) {
                return Err(String::from(
                    "Error connecting to JACK server: Requested client does not exist!",
                ));
            } else if status.intersects(jack::ClientStatus::INVALID_OPTION) {
                return Err(String::from("Error connecting to JACK server: The operation contained an invalid or unsupported option!"));
            }

            return Ok(client);
        }
        Err(e) => {
            return Err(format!("Failed to open client because of error: {:?}", e));
        }
    }
}
