use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use device::{init_devices, Class, Device, Devices};
use stream::PwInitGuard;

use crate::{traits::HostTrait, Error, ErrorKind};

mod device;
mod stream;
mod utils;

/// The PipeWire host, providing access to PipeWire audio devices.
///
/// # PipeWire-Specific Configuration
///
/// PipeWire provides a configuration option to control graph connection behavior:
/// - Port auto-connection via [`set_connect_automatically`](Host::set_connect_automatically)
pub struct Host {
    // Keeps PipeWire initialized for the lifetime of the host, preventing
    // pw_deinit() from running between device enumeration and stream creation.
    _pw: PwInitGuard,
    devices: Vec<Device>,
    connect_automatically: Arc<AtomicBool>,
}

impl Host {
    pub fn new() -> Result<Self, Error> {
        let _pw = PwInitGuard::new();
        let connect_automatically = Arc::new(AtomicBool::new(true));
        let devices = init_devices(connect_automatically.clone()).ok_or_else(|| {
            Error::with_message(ErrorKind::HostUnavailable, "PipeWire is not available")
        })?;
        Ok(Self {
            _pw,
            devices,
            connect_automatically,
        })
    }

    /// Configures whether created streams should automatically connect to system playback/capture
    /// nodes via the session manager.
    ///
    /// When enabled (default), PipeWire's session manager links the stream to the appropriate sink
    /// or source automatically. When disabled, the stream node is registered in the graph but left
    /// unlinked; users must then manually connect ports using PipeWire tools or session manager
    /// APIs.
    ///
    /// Default: `true`
    pub fn set_connect_automatically(&mut self, connect: bool) {
        self.connect_automatically.store(connect, Ordering::Relaxed);
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        utils::find_socket_path().is_some()
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Ok(self.devices.clone().into_iter())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        self.devices
            .iter()
            .find(|device| matches!(device.class(), Class::DefaultInput))
            .cloned()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        self.devices
            .iter()
            .find(|device| matches!(device.class(), Class::DefaultOutput))
            .cloned()
    }
}
