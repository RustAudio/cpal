use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use device::{Class, Device, Devices, Identity, init_devices};
use stream::PwInitGuard;

use crate::{ApplicationInfo, Error, ErrorKind, StreamMetadata, traits::HostTrait};

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
/// - The application's name and icon via [`with_application`](Host::with_application)
/// - A stream's name and role via [`set_stream_metadata`](Host::set_stream_metadata)
pub struct Host {
    // Keeps PipeWire initialized for the lifetime of the host, preventing
    // pw_deinit() from running between device enumeration and stream creation.
    _pw: PwInitGuard,
    devices: Vec<Device>,
    connect_automatically: Arc<AtomicBool>,
    identity: Arc<Mutex<Identity>>,
}

impl Host {
    pub fn new() -> Result<Self, Error> {
        let _pw = PwInitGuard::new();
        let connect_automatically = Arc::new(AtomicBool::new(true));
        let identity = Arc::new(Mutex::new(Identity::default()));
        let devices =
            init_devices(connect_automatically.clone(), identity.clone()).ok_or_else(|| {
                Error::with_message(ErrorKind::HostUnavailable, "PipeWire is not available")
            })?;
        Ok(Self {
            _pw,
            devices,
            connect_automatically,
            identity,
        })
    }

    /// Creates a host whose streams name `application` as their owner.
    ///
    /// Its name and icon become the `application.name` and `application.icon-name` of each
    /// stream and of its client, which volume mixers show instead of the process name.
    ///
    /// Returns [`ErrorKind::InvalidInput`] if a value contains a NUL byte.
    pub fn with_application(application: ApplicationInfo) -> Result<Self, Error> {
        application.validate()?;
        let host = Self::new()?;
        host.identity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .application = Some(application);
        Ok(host)
    }

    /// Describes the streams built from this host's devices from now on.
    ///
    /// The name becomes the stream's `media.name` (cpal's own is `cpal-playback-<pid>` or
    /// `cpal-capture-<pid>`) and the role its `media.role`, which WirePlumber's policies use.
    /// Streams already built keep the metadata they were built with.
    ///
    /// Returns [`ErrorKind::InvalidInput`] if the name contains a NUL byte.
    pub fn set_stream_metadata(&mut self, metadata: StreamMetadata) -> Result<(), Error> {
        metadata.validate()?;
        self.identity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stream = metadata;
        Ok(())
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
