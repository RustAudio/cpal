//! JACK backend implementation.
//!
//! Available on all platforms with the `jack` feature. Requires JACK server and client libraries.

extern crate jack;

use crate::{traits::HostTrait, Error, ErrorKind, SampleFormat};

mod device;
mod stream;

#[allow(unused_imports)] // Re-exported for public API via platform module
pub use self::{
    device::{Device, SupportedInputConfigs, SupportedOutputConfigs},
    stream::Stream,
};

const JACK_SAMPLE_FORMAT: SampleFormat = SampleFormat::F32;

pub type Devices = std::vec::IntoIter<Device>;

/// The JACK host, providing access to JACK audio devices.
///
/// # JACK-Specific Configuration
///
/// Unlike other backends, JACK provides configuration options to control connection and server
/// behavior:
/// - Port auto-connection via [`set_connect_automatically`](Host::set_connect_automatically)
/// - Server auto-start via [`set_start_server_automatically`](Host::set_start_server_automatically)
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
    pub fn new() -> Result<Self, Error> {
        let mut host = Host {
            name: format!("cpal_client_{}", std::process::id()),
            connect_ports_automatically: true,
            start_server_automatically: false,
            devices_created: vec![],
        };
        // Devices don't exist for JACK, they have to be created
        host.initialize_default_devices();
        Ok(host)
    }
    /// Configures whether created ports should automatically connect to system playback/capture
    /// ports.
    ///
    /// When enabled (default), output streams connect to system playback ports and input streams
    /// connect to system capture ports automatically. When disabled, applications must manually
    /// connect ports using JACK tools or APIs.
    ///
    /// Default: `true`
    pub fn set_connect_automatically(&mut self, do_connect: bool) {
        self.connect_ports_automatically = do_connect;
    }

    /// Configures whether the JACK server should automatically start if not already running.
    ///
    /// When enabled, attempting to create a JACK client will start the JACK server if it's not
    /// running. When disabled (default), client creation fails if the server is not running.
    ///
    /// Default: `false`
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

        if let Ok(device) = in_device_res {
            self.devices_created.push(device);
        }

        let out_device_res = Device::default_output_device(
            &self.name,
            self.connect_ports_automatically,
            self.start_server_automatically,
        );
        if let Ok(device) = out_device_res {
            self.devices_created.push(device);
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
    /// There is no way to know if the user has set up a correct JACK configuration e.g. with
    /// qjackctl.
    /// Users can choose to automatically start the server if it isn't already started when
    /// creating a client so checking if the server is running could give a false negative in some
    /// use cases. For these reasons this function should always return true.
    fn is_available() -> bool {
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
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

impl From<jack::Error> for Error {
    fn from(err: jack::Error) -> Self {
        let msg = format!("{err}");
        match err {
            jack::Error::ClientError(_)
            | jack::Error::ClientActivationError
            | jack::Error::ClientDeactivationError
            | jack::Error::LibraryError(_)
            | jack::Error::WeakFunctionNotFound(_)
            | jack::Error::RingbufferCreateFailed => {
                Error::with_message(ErrorKind::DeviceNotAvailable, msg)
            }

            jack::Error::ClientIsNoLongerAlive | jack::Error::ClientPanicked => {
                Error::with_message(ErrorKind::StreamInvalidated, msg)
            }

            jack::Error::SetBufferSizeError | jack::Error::NotEnoughSpace => {
                Error::with_message(ErrorKind::UnsupportedConfig, msg)
            }

            jack::Error::InvalidDeactivation | jack::Error::FreewheelError => {
                Error::with_message(ErrorKind::UnsupportedOperation, msg)
            }

            jack::Error::PortNamingError | jack::Error::PortAliasError => {
                Error::with_message(ErrorKind::InvalidInput, msg)
            }

            _ => Error::with_message(ErrorKind::Other, msg),
        }
    }
}

fn get_client(name: &str, client_options: jack::ClientOptions) -> Result<jack::Client, Error> {
    let (client, status) = jack::Client::new(name, client_options)?;
    if status.intersects(jack::ClientStatus::VERSION_ERROR) {
        Err(Error::with_message(
            ErrorKind::UnsupportedOperation,
            "client protocol version does not match the JACK server",
        ))
    } else if status.intersects(jack::ClientStatus::INVALID_OPTION) {
        Err(Error::with_message(
            ErrorKind::UnsupportedOperation,
            "JACK client operation contained an invalid or unsupported option",
        ))
    } else if status.intersects(jack::ClientStatus::SERVER_ERROR) {
        Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "error communicating with the JACK server",
        ))
    } else if status.intersects(jack::ClientStatus::SERVER_FAILED) {
        Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "could not connect to the JACK server",
        ))
    } else if status.intersects(jack::ClientStatus::INIT_FAILURE) {
        Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "unable to initialize JACK client",
        ))
    } else if status.intersects(jack::ClientStatus::SHM_FAILURE) {
        Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "unable to access JACK shared memory",
        ))
    } else if status.intersects(jack::ClientStatus::NO_SUCH_CLIENT) {
        Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "requested JACK client does not exist",
        ))
    } else {
        Ok(client)
    }
}
