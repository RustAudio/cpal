use super::utils::{CpalResult, Decorate, err, sample_format_asio2cpal};
use crate::ErrorKind::UnsupportedConfig;
use crate::*;
use azo::driver::Driver;
use azo::dto::ChannelId;
use tap::Pipe;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Config {
    pub format  : SampleFormat,
    pub channels: u16,
    pub input   : bool,
}

impl Config {
    pub fn validate(self, driver: &impl Driver) -> impl Iterator<Item = CpalResult<ChannelId>> {
        (0..self.channels)
            .map(move |i| {
                let id = ChannelId {
                    input: self.input,
                    index: i as _
                };

                let actual_format = driver
                    .channel_info(id)
                    .decorate(driver, stringify!(Driver::channel_info))?
                    .sample_type
                    .pipe(sample_format_asio2cpal);

                if actual_format != Some(self.format) {
                    return err(UnsupportedConfig, "Sample format mismatch");
                }
                Ok(id)
            })
    }
}
