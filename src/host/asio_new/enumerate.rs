use super::*;

#[derive(Debug, Clone)]
pub struct Sessions(worker::Handle, vec::IntoIter<azo::DriverMetadata>);

impl Sessions {
    pub fn new(com_worker: worker::Handle) -> azo::WinResult<Self> {
        let metas = azo::get_drivers()?.into_iter();

        Ok(Self(com_worker, metas))
    }
}

impl Iterator for Sessions {
    type Item = Session;

    fn next(&mut self) -> Option<Self::Item> {
        self.1.find_map(|metadata| Session::try_new(metadata.clsid, &self.0).ok())
    }
}

pub struct Devices(pub(super) Sessions);

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(Device::new)
    }
}

pub type SupportedConfigs = vec::IntoIter<SupportedStreamConfigRange>;
