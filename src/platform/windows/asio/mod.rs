extern crate asio_sys as sys;

pub struct Devices{
}

pub struct Device;

impl Default for Devices {
    fn default() -> Devices {
        Devices{}
    }
}

impl Device {
    pub fn name(&self) -> String {
        "".to_owned()
    }
}

