use super::utils::create_report;
use crate::ErrorKind::*;
use crate::*;
use azo::Driver;
use azo::dto::{ChannelCounts, ChannelId};
use std::collections::HashSet;
use tap::Pipe;

use super::{CpalResult, err, sample_format_asio2cpal};

pub fn channel_count<const INPUT: bool>(driver: &Driver) -> CpalResult<i32> {
    channel_counts(driver).map(|counts| if INPUT { counts.in_ } else { counts.out })
}

pub fn channel_counts(driver: &Driver) -> CpalResult<ChannelCounts> {
    driver.channel_counts().map_err(|error| {
        Error::with_message(
            BackendError,
            format!("failed to retrieve channel coounts: {error}"),
        )
    })
}

pub fn sample_rates(driver: &Driver) -> CpalResult<(SampleRate, SampleRate)> {
    let mut rates_iter = COMMON_SAMPLE_RATES
        .iter()
        .copied()
        .filter(|rate| driver.can_sample_rate(*rate as _).is_ok());

    let min = rates_iter.next().ok_or(Error::with_message(
        DeviceNotAvailable,
        "no supported sample rate found",
    ))?;
    let max = rates_iter.next_back().unwrap_or(min);

    Ok((min, max))
}

pub fn supported_buffer_size(driver: &Driver) -> SupportedBufferSize {
    use crate::SupportedBufferSize::*;

    driver.buffer_size().map_or(Unknown, |bs| Range {
        min: bs.min as _,
        max: bs.max as _,
    })
}

pub fn preferred_buffer_size(driver: &Driver) -> CpalResult<i32> {
    let value = driver
        .buffer_size()
        .map_err(|error| create_report(driver, error, stringify!(Driver::buffer_size)))?
        .preferred;

    if value.is_negative() {
        return err(
            BackendError,
            format!("ASIO driver reported invalid buffer size {value}"),
        );
    }

    Ok(value)
}

pub fn sample_formats<const INPUT: bool>(
    driver: &Driver,
    ch_count: i32,
) -> CpalResult<impl Iterator<Item = SampleFormat>> {
    (0..ch_count)
        .map(move |index| {
            driver
                .channel_info(ChannelId {
                    index,
                    input: INPUT,
                })
                .map(|ch_info| ch_info.sample_type)
                .map_err(|error| create_report(driver, error, stringify!(Driver::channel_info)))
        })
        .collect::<CpalResult<HashSet<_>>>()? // aggregates errors and deduplicates the values
        .into_iter()
        .filter_map(sample_format_asio2cpal)
        .pipe(Ok)
}
