use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use device::{Class, Device, Devices, init_devices};
use stream::PwInitGuard;

use crate::{Error, ErrorKind, traits::HostTrait};

mod device;
#[cfg(all(target_os = "linux", feature = "realtime"))]
mod rt_promote;
mod stream;
mod utils;

/// The PipeWire host, providing access to PipeWire audio devices.
///
/// # PipeWire-Specific Configuration
///
/// PipeWire provides configuration options specific to this backend:
/// - Port auto-connection via [`set_connect_automatically`](Host::set_connect_automatically)
/// - Custom stream node properties via [`set_stream_properties`](Host::set_stream_properties)
pub struct Host {
    // Keeps PipeWire initialized for the lifetime of the host, preventing
    // pw_deinit() from running between device enumeration and stream creation.
    _pw: PwInitGuard,
    devices: Vec<Device>,
    connect_automatically: Arc<AtomicBool>,
    stream_properties: Arc<Mutex<Vec<(String, String)>>>,
}

impl Host {
    pub fn new() -> Result<Self, Error> {
        let _pw = PwInitGuard::new();
        let connect_automatically = Arc::new(AtomicBool::new(true));
        let stream_properties = Arc::new(Mutex::new(Vec::new()));
        let devices = init_devices(connect_automatically.clone(), stream_properties.clone())
            .ok_or_else(|| {
                Error::with_message(ErrorKind::HostUnavailable, "PipeWire is not available")
            })?;
        Ok(Self {
            _pw,
            devices,
            connect_automatically,
            stream_properties,
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

    /// Sets custom properties on stream nodes created by this host.
    ///
    /// User properties override cpal's defaults for matching keys (e.g. `node.name`, `media.name`).
    /// Calling this replaces any properties set by a previous call.
    pub fn set_stream_properties<I: IntoIterator<Item = (String, String)>>(
        &mut self,
        properties: I,
    ) {
        let mut guard = self
            .stream_properties
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = properties.into_iter().collect();
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
