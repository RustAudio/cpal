//! Implementations for ASIO-specific device functionality.

#[allow(unused_imports)]
use crate::BackendSpecificError;
use crate::Device;
#[allow(unused_imports)]
use std::marker::PhantomData;

/// Extension trait to get the ASIO device.
pub trait AsioDeviceExt {
    /// Returns the [AsioDevice] interface if this is an ASIO device.
    fn as_asio(&self) -> Option<AsioDevice<'_>>;
}

/// A wrapper providing access to ASIO-specific device functionality.
pub struct AsioDevice<'a> {
    #[cfg(all(target_os = "windows", feature = "asio"))]
    inner: &'a crate::host::asio::Device,

    // Dummy marker for lifetime 'a.
    #[cfg(not(all(target_os = "windows", feature = "asio")))]
    _marker: PhantomData<&'a ()>,
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
            use crate::host::asio::GLOBAL_ASIO;

            let description = self.inner.description().map_err(|e| BackendSpecificError {
                description: format!("{e:?}"),
            })?;
            let driver_name = description.name();

            GLOBAL_ASIO
                .get()
                .expect("GLOBAL_ASIO is always set when an ASIO device exists")
                .load_driver(driver_name)
                .map_err(|e| BackendSpecificError {
                    description: format!("{e:?}"),
                })?
                .open_control_panel()
                .map_err(|e| BackendSpecificError {
                    description: format!("{e:?}"),
                })
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
