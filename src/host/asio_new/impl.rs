use crate::ErrorKind::*;
use crate::*;
use azo::Driver;
use azo::dto::{ChannelCounts, ChannelId};
use std::fmt::{self, Debug};
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::vec;
use tap::prelude::*;
use windows_core::{GUID, Interface};

mod capabilities;
mod com_worker;
mod ffi_callbacks;

use super::utils::*;

use self::ffi_callbacks::DoubleBuffer;

#[derive(Debug, Clone)]
pub struct Host(com_worker::Link);

impl Host {
    #[must_use]
    pub(crate) fn is_available_() -> bool {
        // this will return false if the ASIO registry keys are either
        // * missing - meaning no ASIO driver has ever been installed on the system
        // * corrupted - in which case ASIO is unusable
        azo::get_drivers().is_ok()
    }

    pub(crate) fn devices_(&self) -> CpalResult<DeviceIter> {
        DeviceIter::new(self.0.clone())
            .map_err(|_win_error| Error::new(HostUnavailable))
    }

    #[must_use]
    pub(crate) fn default_device_(&self, filter: fn(&Device) -> bool) -> Option<Device> {
        self.devices_()
            .ok()?
            .find(filter)
    }

    #[must_use]
    pub(crate) fn device_by_id_(&self, id: &DeviceId) -> Option<Device> {
        if id.host() != HostId::AsioNew {
            return None;
        }

        let clsid = id.id().try_into().ok()?;

        Device::new(clsid, &self.0).ok()
    }
}

impl Host {
    pub fn new() -> CpalResult<Self> {
        com_worker::Link
            ::new()
            .pipe(Self)
            .pipe(Ok)
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug, Clone)]
pub struct DeviceIter(com_worker::Link, vec::IntoIter<azo::DriverMetadata>);

impl DeviceIter {
    fn new(com_worker: com_worker::Link) -> azo::WinResult<Self> {
        let metas = azo::get_drivers()?.into_iter();

        Ok(Self(com_worker, metas))
    }
}

impl Iterator for DeviceIter {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.1.find_map(|metadata| Device::new(metadata.clsid, &self.0).ok())
    }
}

////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////

pub type SupportedConfigs = vec::IntoIter<SupportedStreamConfigRange>;

#[derive(Debug)]
pub struct Device {
    driver: Driver,
    init_success: bool,
    clsid_string: String,
    _com_worker: com_worker::Link
}

impl Device {
    pub(crate) fn new(clsid: GUID, com_worker: &com_worker::Link) -> azo::WinResult<Self> {
        let driver = com_worker.create_driver(clsid)?;

        Self {
            init_success: driver.init(None),
            driver,
            clsid_string: format!("{clsid:?}"),
            _com_worker: com_worker.clone(), // hold on to this to keep the thread alive that initialized the COM apartment in which the driver was created
        }
        .pipe(Ok)
    }

    pub(crate) fn id(&self) -> CpalResult<DeviceId> {
        DeviceId::new(
            HostId::AsioNew,
            self.clsid_string.clone()
        )
        .pipe(Ok)
    }

    pub(crate) fn description(&self) -> CpalResult<DeviceDescription> {
        let name_c = self.driver.name();
        let name = name_c.to_string_lossy();

        let direction = match self.driver.channel_counts() {
            Ok(ChannelCounts { in_: 1.., out: 1.. }) => DeviceDirection::Duplex,
            Ok(ChannelCounts { in_: 1.., out: 0   }) => DeviceDirection::Input,
            Ok(ChannelCounts { in_: 0  , out: 1.. }) => DeviceDirection::Output,
            _                                        => DeviceDirection::Unknown,
        };

        let mut extended = vec![format!("driver version: {}", self.driver.version())];

        if !self.init_success {
            extended.push("driver failed to initialize".to_owned()); // ASIO drivers can often still do *something* when they fail to initialize
            extended.push(format!(
                "last error: {}",
                self.driver.last_error().to_string_lossy()
            ));
        }

        DeviceDescriptionBuilder
            ::new(&name)
            .driver(name)
            .direction(direction)
            .extended(extended)
            .build()
            .pipe(Ok)
    }

    #[must_use]
    pub(crate) fn supports_direction<const IN: bool, const OUT: bool>(&self) -> bool {
        if !self.init_success {
            return false;
        }

        let Ok(counts) = self.driver.channel_counts()
        else { return false; }; // can't do anything if it can't even count the channels

        if IN && counts.in_ == 0 {
            return false;
        }

        if OUT && counts.out == 0 {
            return false;
        }

        true
    }

    pub(crate) fn supported_configs<const INPUT: bool>(&self) -> CpalResult<SupportedConfigs> {
        let ch_count = capabilities::channel_count::<INPUT>(&self.driver)?;
        if ch_count == 0 {
            return err(UnsupportedOperation, "the device has no channels in this direction");
        }

        let (min_rate, max_rate) = capabilities::sample_rates(&self.driver)?;
        let buf_size             = capabilities::buffer_size_supported(&self.driver);
        let sample_formats       = capabilities::sample_formats::<INPUT>(&self.driver, ch_count)?;

        sample_formats
            .map(move |format| SupportedStreamConfigRange::new(ch_count as _, min_rate, max_rate, buf_size, format))
            .collect::<Vec<_>>()
            .into_iter()
            .pipe(Ok)
    }

    pub(crate) fn default_config<const INPUT: bool>(&self) -> CpalResult<SupportedStreamConfig> {
        self.supported_configs::<INPUT>()?
            .next()
            .expect("infallible")
            .pipe(|range|
                SupportedStreamConfig::new(
                    range.channels(),
                    range.min_sample_rate(),
                    *range.buffer_size(),
                    range.sample_format()
                )
            )
            .pipe(Ok)
    }

    pub(crate) fn build_stream_raw(
        self: &Arc<Self>,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
        timeout    : Option<Duration>,
        sample_rate: SampleRate,
        buffer_size: BufferSize,
        directional: [Option<StreamDirectionalArgs>; 2]
    ) -> CpalResult<Stream> {
        Stream::new(
            self,
            data_cb,
            error_cb,
            timeout,
            sample_rate,
            buffer_size,
            directional
        )?
        .pipe(Ok)
    }

    pub(crate) fn fmt_(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.driver.name().to_string_lossy())
    }
}

impl Hash for Device {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.driver.as_raw().as_raw().hash(state);
        self.init_success.hash(state);
        self.clsid_string.hash(state);
        // self._com_worker.hash(state);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////////////////////
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct Stream {
    device: Arc<Device>,
    buffer_size: FrameCount,
    _ffi_callbacks: Pin<Box<ffi_callbacks::Container>>
}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl Stream {
    pub(crate) fn new(
        device     : &Arc<Device>,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
        _timeout   : Option<Duration>,
        sample_rate: SampleRate,
        buffer_size: BufferSize,
        directional: [Option<StreamDirectionalArgs>; 2]
    )
    -> CpalResult<Self>
    {
        // Since there is no channel map in cpal's API (yet), channel selection is always 0..n.
        // And since ASIO channels are always mono, this effectively means that, at least currently,
        // only the first channel in each direction can be used at all
        let channel_ids = directional
            .iter()
            .zip([true, false])
            .filter_map(|(option, direction)|
                option.map(|args| args.validate(&device.driver, direction))
            )
            .collect::<CpalResult<Vec<_>>>()?; // not a hot path, so should be fine

        let buffer_size_final = match buffer_size {
            BufferSize::Fixed(n) => n,
            BufferSize::Default  => capabilities::buffer_size_preferred(&device.driver)?,
        };

        device
            .driver
            .can_sample_rate(sample_rate as _)
            .map_err(|_| Error::with_message(InvalidInput, "sample rate not supported"))?;

        device
            .driver
            .set_sample_rate(sample_rate as _)
            .map_err(|azo_error| create_report(&device.driver, azo_error, "set_sample_rate"))?;

        let mut ffi_callbacks = ffi_callbacks::Container
            ::default() // creates dummy callbacks that do nothing
            .pipe(Box::pin);

        let mut double_buffers =
            unsafe { device.driver.create_buffers(channel_ids, buffer_size_final as _, ffi_callbacks.pointers()) }
            .map_err(|error| create_report(&device.driver, error, "create_buffers"))?
            .map(DoubleBuffer);

        let ffi_directional_args = directional.map(|option|
            option.map(|prep_args| ffi_callbacks::DirectionalArgs {
                format: prep_args.format,
                double_buffer: double_buffers.next().unwrap()
            })
        );

        // The real callbacks can only be created *after* passing `callbacks.pointers` to
        // `create_buffers()`, as the buffer swap callback needs to capture the buffer pointers
        ffi_callbacks.as_mut().update(
            Arc::clone(device),
            data_cb,
            error_cb,
            buffer_size_final as _,
            ffi_directional_args
        );

        Self {
            device: Arc::clone(device),
            buffer_size: buffer_size_final,
            _ffi_callbacks: ffi_callbacks // keep this alive until the whole stream is dropped
        }
        .pipe(Ok)
    }

    pub(crate) fn resume(&self) -> CpalResult<()> {
        self.device
            .driver
            .start()
            .map_err(|error| create_report(&self.device.driver, error, "start"))
    }

    pub(crate) fn halt(&self) -> CpalResult<()> {
        self.device
            .driver
            .stop()
            .map_err(|error| create_report(&self.device.driver, error, "stop"))
    }

    pub(crate) fn now(&self) -> StreamInstant {
        self.device
            .driver
            .sample_position()
            .map_or(0, |pos| pos.time_stamp as u64)
            .pipe(StreamInstant::from_millis)
    }

    #[expect(clippy::unnecessary_wraps, reason = "consistency")]
    pub const fn buffer_size(&self) -> CpalResult<FrameCount> {
        Ok(self.buffer_size)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        _ = self.halt(); // might fail if the stream is already halted
        _ = self.device.driver.dispose_all_buffers(); // dunno in what kind of scenario this would fail
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamDirectionalArgs {
    pub format: SampleFormat,
    pub channels: u16
}

impl StreamDirectionalArgs {
    pub(crate) fn validate(self, driver: &Driver, input: bool) -> CpalResult<ChannelId> {
        let channel_id = match self.channels {
            0   => err(InvalidInput, "need at least 1 channel"),
            1   => Ok(ChannelId { index: 0, input }),
            2.. => err(UnsupportedConfig, "ASIO only supports mono streams")
        }?;

        let actual_format = driver
            .channel_info(channel_id)
            .map_err(|error| create_report(driver, error, "buffer_size"))?
            .sample_type
            .pipe(sample_format_azo2cpal);

        if actual_format != Some(self.format) {
            return err(UnsupportedConfig, "Sample format mismatch");
        }

        Ok(channel_id)
    }
}
