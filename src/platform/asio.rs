//! Implementations for ASIO-specific device functionality.

use crate::BackendSpecificError;
use crate::Device;

/// Extension trait to get the ASIO device.
pub trait AsioDeviceExt {
    /// Returns the [AsioDevice] interface if this is an ASIO device.
    fn as_asio(&self) -> Option<AsioDevice<'_>>;
}

/// A wrapper providing access to ASIO-specific device functionality.
#[derive(Clone)]
pub struct AsioDevice<'a> {
    #[cfg(all(target_os = "windows", feature = "asio"))]
    inner: &'a crate::host::asio::Device,

    // Dummy marker for lifetime 'a.
    #[cfg(not(all(target_os = "windows", feature = "asio")))]
    _marker: std::marker::PhantomData<&'a ()>,
}

impl AsioDevice<'_> {
    /// Opens the ASIO driver's control panel window.
    ///
    /// This provides access to device-specific settings like buffer size,
    /// sample rate, input/output routing, and hardware-specific features.
    ///
    /// # Blocking Behavior
    ///
    /// This call may block until the user closes the control panel.
    /// Consider spawning a thread to avoid blocking the main thread.
    pub fn open_control_panel(&self) -> Result<(), BackendSpecificError> {
        #[cfg(all(target_os = "windows", feature = "asio"))]
        {
            self.inner.open_control_panel()
        }

        #[cfg(not(all(target_os = "windows", feature = "asio")))]
        unreachable!("AsioDevice cannot be constructed on non-ASIO platforms")
    }
}

impl AsioDeviceExt for Device {
    fn as_asio(&self) -> Option<AsioDevice<'_>> {
        match self.as_inner() {
            #[cfg(all(target_os = "windows", feature = "asio"))]
            crate::platform::DeviceInner::Asio(d) => Some(AsioDevice { inner: d }),
            _ => None,
        }
    }
}
