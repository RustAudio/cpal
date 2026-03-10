//! Implementations for ASIO-specific device functionality.

use crate::BackendSpecificError;
use crate::Device;

/// Extension trait for ASIO-specific device functionality.
pub trait AsioDeviceExt {
    /// Returns `true` if this device is an ASIO device.
    fn is_asio_device(&self) -> bool;

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
    fn asio_open_control_panel(&self) -> Result<(), BackendSpecificError>;
}

#[cfg(all(target_os = "windows", feature = "asio"))]
impl AsioDeviceExt for Device {
    fn is_asio_device(&self) -> bool {
        matches!(self.as_inner(), crate::platform::DeviceInner::Asio(_))
    }

    fn asio_open_control_panel(&self) -> Result<(), BackendSpecificError> {
        use crate::host::asio::GLOBAL_ASIO;
        use crate::platform::DeviceInner;

        if let DeviceInner::Asio(ref asio_device) = self.as_inner() {
            let description = asio_device
                .description()
                .map_err(|e| BackendSpecificError {
                    description: format!("Could not get device name: {:?}", e),
                })?;

            let driver = GLOBAL_ASIO
                .get()
                .ok_or(BackendSpecificError {
                    description: "ASIO not initialized.".into(),
                })?
                .load_driver(description.name())
                .map_err(|e| BackendSpecificError {
                    description: format!("Failed to load driver: {:?}", e),
                })?;

            driver
                .open_control_panel()
                .map_err(|e| BackendSpecificError {
                    description: format!("Failed to open control panel: {:?}", e),
                })
        } else {
            Err(BackendSpecificError {
                description: "Not an ASIO device".to_string(),
            })
        }
    }
}

#[cfg(not(all(target_os = "windows", feature = "asio")))]
impl AsioDeviceExt for Device {
    fn is_asio_device(&self) -> bool {
        false
    }

    fn asio_open_control_panel(&self) -> Result<(), BackendSpecificError> {
        Err(not_available())
    }
}

#[cfg(not(all(target_os = "windows", feature = "asio")))]
fn not_available() -> BackendSpecificError {
    BackendSpecificError {
        description: "ASIO is not available on this platform".to_string(),
    }
}
