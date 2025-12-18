use crate::platform::DeviceInner;
use crate::BackendSpecificError;
use crate::Device;

pub trait AsioDeviceExt {
    fn asio_open_control_panel(&self) -> Result<(), BackendSpecificError>;
}

impl AsioDeviceExt for Device {
    fn asio_open_control_panel(&self) -> Result<(), BackendSpecificError> {
        if let DeviceInner::Asio(ref asio_device) = self.as_inner() {
            asio_device
                .driver
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
