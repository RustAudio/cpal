extern crate jack;

use crate::{DevicesError, SampleFormat, SupportedStreamConfigRange};
use traits::HostTrait;

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
    devices_created: Vec<Device>,
}

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        let mut host = Host {
            name: "cpal_client".to_owned(),
            connect_ports_automatically: true,
            start_server_automatically: false,
            devices_created: vec![],
        };
        // Devices don't exist for JACK, they have to be created
        host.initialize_default_devices();
        Ok(host)
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

    fn initialize_default_devices(&mut self) {
        let in_device_res = Device::default_input_device(
            &self.name,
            self.connect_ports_automatically,
            self.start_server_automatically,
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
            self.start_server_automatically,
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

    /// JACK is available if
    /// - the jack feature flag is set
    /// - libjack is installed (wouldn't compile without it)
    /// - the JACK server can be started
    ///
    /// If the code compiles the necessary jack libraries are installed.
    /// There is no way to know if the user has set up a correct JACK configuration e.g. with qjackctl.
    /// Users can choose to automatically start the server if it isn't already started when creating a client
    /// so checking if the server is running could give a false negative in some use cases.
    /// For these reasons this function should always return true.
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
