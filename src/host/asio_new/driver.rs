use super::SupportedConfigs;
use super::buffer;
use super::utils::CpalResult;
use crate::*;
use std::sync::Arc;
use std::time::Duration;

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
        self       : &Arc<Self>,
        cfg_in     : buffer::Config,
        cfg_out    : buffer::Config,
        sample_rate: SampleRate,
        buffer_size: BufferSize,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
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
