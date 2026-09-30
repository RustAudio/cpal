use std::time::Duration;

use crate::*;
use super::SupportedConfigs;
use super::utils::CpalResult;

#[derive(Debug)]
pub struct Session;

impl Session {
    pub fn id(&self) -> DeviceId {
        todo!()
    }

    pub fn display_name(&self) -> String {
        todo!()
    }

    pub fn description(&self) -> CpalResult<DeviceDescription> {
        todo!()
    }

    #[must_use]
    pub fn supports_direction<const IN: bool, const OUT: bool>(&self) -> bool {
        todo!()
    }

    pub fn supported_configs<const INPUT: bool>(&self) -> CpalResult<SupportedConfigs> {
        todo!()
    }

    pub fn default_config<const INPUT: bool>(&self) -> CpalResult<SupportedStreamConfig> {
        todo!()
    }

	pub fn build_stream(
		
    ) -> CpalResult<super::Stream> {
        todo!()
    }

    pub fn start(&self) -> CpalResult<()> {
        todo!()
    }

    pub fn pause(&self) -> CpalResult<()> {
        todo!()
    }

    pub fn stop(&self, max_wait: Option<Duration>) -> CpalResult<()> {
        todo!()
    }

    pub fn now(&self) -> StreamInstant {
        todo!()
    }

    pub fn reset(&self) -> CpalResult<()> {
        todo!()
    }
}
