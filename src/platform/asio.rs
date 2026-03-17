//! Implementations for ASIO-specific device functionality.

#[allow(unused_imports)]
use crate::BackendSpecificError;
use crate::Device;

/// Extension trait to get ASIO device.
pub trait AsioDeviceExt {
    fn as_asio(&self) -> Option<AsioDevice<'_>>;
}

/// Struct containing for ASIO-specific device functionality.
#[cfg(all(target_os = "windows", feature = "asio"))]
pub struct AsioDevice<'a>(&'a crate::host::asio::Device);

#[cfg(not(all(target_os = "windows", feature = "asio")))]
pub struct AsioDevice<'a>(std::marker::PhantomData<&'a ()>);

#[cfg(all(target_os = "windows", feature = "asio"))]
impl AsioDevice<'_> {
    /// Opens the ASIO driver's control panel window.
    ///
    /// This provides access to device-specific settings like buffer size,
    /// sample rate, input/output routing, and hardware-specific features.
    ///
    /// # Blocking Behavior
    ///
    /// **WARNING**: This call may block until the user closes the control panel.
    /// Consider spawning a thread to avoid blocking the main thread.
    ///
    /// # Errors
    ///
    /// Returns an error if this device is not an ASIO device.
    pub fn open_control_panel(&self) -> Result<(), BackendSpecificError> {
        use crate::host::asio::GLOBAL_ASIO;

        let description = self.0.description().map_err(|e| BackendSpecificError {
            description: format!("{e:?}"),
        })?;
        let driver_name = description.name();

        GLOBAL_ASIO
            .get()
            .expect("GLOBAL_ASIO is always set when an ASIO Device exists")
            .load_driver(driver_name)
            .map_err(|e| BackendSpecificError {
                description: format!("{e:?}"),
            })?
            .open_control_panel()
            .map_err(|e| BackendSpecificError {
                description: format!("{e:?}"),
            })
    }
}

impl AsioDeviceExt for Device {
    fn as_asio(&self) -> Option<AsioDevice<'_>> {
        match self.as_inner() {
            #[cfg(all(target_os = "windows", feature = "asio"))]
            crate::platform::DeviceInner::Asio(d) => Some(AsioDevice(d)),
            _ => None,
        }
    }
}
