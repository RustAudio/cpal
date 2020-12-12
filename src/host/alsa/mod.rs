extern crate alsa;
extern crate libc;
extern crate parking_lot;

use self::parking_lot::Mutex;
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, ChannelCount, Data,
    DefaultStreamConfigError, DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo,
    PauseStreamError, PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use std::cmp;
use std::convert::TryInto;
use std::sync::{atomic, Arc};
use std::thread::{self, JoinHandle};
use std::vec::IntoIter as VecIntoIter;
use traits::{DeviceTrait, HostTrait, StreamTrait};

pub use self::enumerate::{default_input_device, default_output_device, Devices};

pub type SupportedInputConfigs = VecIntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = VecIntoIter<SupportedStreamConfigRange>;

mod enumerate;

/// The default linux, dragonfly and freebsd host type.
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume ALSA is always available on linux/dragonfly/freebsd.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Device::supported_input_configs(self)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Device::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_output_config(self)
    }

    fn supports_input(&self) -> bool {
        self.direction != Some(alsa::Direction::Playback)
    }

    fn supports_output(&self) -> bool {
        self.direction != Some(alsa::Direction::Capture)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Capture)?;
        let stream = Stream::new_input(Arc::new(stream_inner), data_callback, error_callback);
        Ok(stream)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Playback)?;
        let stream = Stream::new_output(Arc::new(stream_inner), data_callback, error_callback);
        Ok(stream)
    }
}

#[derive(Default)]
struct DeviceHandles {
    playback: Option<alsa::PCM>,
    capture: Option<alsa::PCM>,
}

impl DeviceHandles {
    /// Get a mutable reference to the `Option` for a specific `stream_type`.
    /// If the `Option` is `None`, the `alsa::PCM` will be opened and placed in
    /// the `Option` before returning. If `handle_mut()` returns `Ok` the contained
    /// `Option` is guaranteed to be `Some(..)`.
    fn try_open(
        &mut self,
        name: &str,
        stream_type: alsa::Direction,
    ) -> Result<&mut Option<alsa::PCM>, alsa::Error> {
        let handle = match stream_type {
            alsa::Direction::Playback => &mut self.playback,
            alsa::Direction::Capture => &mut self.capture,
        };

        if handle.is_none() {
            *handle = Some(alsa::pcm::PCM::new(name, stream_type, true)?);
        }

        Ok(handle)
    }

    /// Get a mutable reference to the `alsa::PCM` handle for a specific `stream_type`.
    /// If the handle is not yet opened, it will be opened and stored in `self`.
    fn get_mut(
        &mut self,
        name: &str,
        stream_type: alsa::Direction,
    ) -> Result<&mut alsa::PCM, alsa::Error> {
        Ok(self.try_open(name, stream_type)?.as_mut().unwrap())
    }

    /// Take ownership of the `alsa::PCM` handle for a specific `stream_type`.
    /// If the handle is not yet opened, it will be opened and returned.
    fn take(&mut self, name: &str, stream_type: alsa::Direction) -> Result<alsa::PCM, alsa::Error> {
        Ok(self.try_open(name, stream_type)?.take().unwrap())
    }
}

pub struct Device {
    name: String,
    direction: Option<alsa::Direction>,
    handles: Mutex<DeviceHandles>,
}

impl Default for Device {
    fn default() -> Self {
        Device {
            name: "default".to_string(),
            direction: None,
            handles: Default::default(),
        }
    }
}

impl Device {
    fn build_stream_inner(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        stream_type: alsa::Direction,
    ) -> Result<StreamInner, BuildStreamError> {
        let handle = self.handles.lock().take(&self.name, stream_type)?;

        let can_pause = set_hw_params_from_format(&handle, conf, sample_format)?;
        let period_len = set_sw_params_from_format(&handle, conf, stream_type)?;

        handle.prepare()?;

        let clock = StreamClock::new(&handle.status()?, conf.sample_rate);

        let stream_inner = StreamInner {
            channel: handle,
            sample_format,
            conf: conf.clone(),
            period_len,
            can_pause,
            clock,
            dropping: atomic::AtomicBool::new(false),
        };

        Ok(stream_inner)
    }

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.name.clone())
    }

    fn supported_configs(
        &self,
        stream_t: alsa::Direction,
    ) -> Result<VecIntoIter<SupportedStreamConfigRange>, SupportedStreamConfigsError> {
        let mut guard = self.handles.lock();
        let handle = guard.get_mut(&self.name, stream_t)?;

        let hw_params = alsa::pcm::HwParams::any(&handle)?;

        // TODO: check endianess
        const FORMATS: [(SampleFormat, alsa::pcm::Format); 3] = [
            //SND_PCM_FORMAT_S8,
            //SND_PCM_FORMAT_U8,
            (SampleFormat::I16, alsa::pcm::Format::S16LE),
            //SND_PCM_FORMAT_S16_BE,
            (SampleFormat::U16, alsa::pcm::Format::U16LE),
            //SND_PCM_FORMAT_U16_BE,
            //SND_PCM_FORMAT_S24_LE,
            //SND_PCM_FORMAT_S24_BE,
            //SND_PCM_FORMAT_U24_LE,
            //SND_PCM_FORMAT_U24_BE,
            //SND_PCM_FORMAT_S32_LE,
            //SND_PCM_FORMAT_S32_BE,
            //SND_PCM_FORMAT_U32_LE,
            //SND_PCM_FORMAT_U32_BE,
            (SampleFormat::F32, alsa::pcm::Format::FloatLE),
            //SND_PCM_FORMAT_FLOAT_BE,
            //SND_PCM_FORMAT_FLOAT64_LE,
            //SND_PCM_FORMAT_FLOAT64_BE,
            //SND_PCM_FORMAT_IEC958_SUBFRAME_LE,
            //SND_PCM_FORMAT_IEC958_SUBFRAME_BE,
            //SND_PCM_FORMAT_MU_LAW,
            //SND_PCM_FORMAT_A_LAW,
            //SND_PCM_FORMAT_IMA_ADPCM,
            //SND_PCM_FORMAT_MPEG,
            //SND_PCM_FORMAT_GSM,
            //SND_PCM_FORMAT_SPECIAL,
            //SND_PCM_FORMAT_S24_3LE,
            //SND_PCM_FORMAT_S24_3BE,
            //SND_PCM_FORMAT_U24_3LE,
            //SND_PCM_FORMAT_U24_3BE,
            //SND_PCM_FORMAT_S20_3LE,
            //SND_PCM_FORMAT_S20_3BE,
            //SND_PCM_FORMAT_U20_3LE,
            //SND_PCM_FORMAT_U20_3BE,
            //SND_PCM_FORMAT_S18_3LE,
            //SND_PCM_FORMAT_S18_3BE,
            //SND_PCM_FORMAT_U18_3LE,
            //SND_PCM_FORMAT_U18_3BE,
        ];

        let mut supported_formats = Vec::new();
        for &(sample_format, alsa_format) in FORMATS.iter() {
            if hw_params.test_format(alsa_format).is_ok() {
                supported_formats.push(sample_format);
            }
        }

        let min_rate = hw_params.get_rate_min()?;
        let max_rate = hw_params.get_rate_max()?;

        let sample_rates = if min_rate == max_rate || hw_params.test_rate(min_rate + 1).is_ok() {
            vec![(min_rate, max_rate)]
        } else {
            const RATES: [libc::c_uint; 13] = [
                5512, 8000, 11025, 16000, 22050, 32000, 44100, 48000, 64000, 88200, 96000, 176400,
                192000,
            ];

            let mut rates = Vec::new();
            for &rate in RATES.iter() {
                if hw_params.test_rate(rate).is_ok() {
                    rates.push((rate, rate));
                }
            }

            if rates.is_empty() {
                vec![(min_rate, max_rate)]
            } else {
                rates
            }
        };

        let min_channels = hw_params.get_channels_min()?;
        let max_channels = hw_params.get_channels_max()?;

        let max_channels = cmp::min(max_channels, 32); // TODO: limiting to 32 channels or too much stuff is returned
        let supported_channels = (min_channels..max_channels + 1)
            .filter_map(|num| {
                if hw_params.test_channels(num).is_ok() {
                    Some(num as ChannelCount)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let min_buffer_size = hw_params.get_buffer_size_min()?;
        let max_buffer_size = hw_params.get_buffer_size_max()?;

        let buffer_size_range = SupportedBufferSize::Range {
            min: min_buffer_size as u32,
            max: max_buffer_size as u32,
        };

        let mut output = Vec::with_capacity(
            supported_formats.len() * supported_channels.len() * sample_rates.len(),
        );
        for &sample_format in supported_formats.iter() {
            for &channels in supported_channels.iter() {
                for &(min_rate, max_rate) in sample_rates.iter() {
                    output.push(SupportedStreamConfigRange {
                        channels,
                        min_sample_rate: SampleRate(min_rate as u32),
                        max_sample_rate: SampleRate(max_rate as u32),
                        buffer_size: buffer_size_range.clone(),
                        sample_format,
                    });
                }
            }
        }

        Ok(output.into_iter())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        self.supported_configs(alsa::Direction::Capture)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        self.supported_configs(alsa::Direction::Playback)
    }

    // ALSA does not offer default stream formats, so instead we compare all supported formats by
    // the `SupportedStreamConfigRange::cmp_default_heuristics` order and select the greatest.
    fn default_config(
        &self,
        stream_t: alsa::Direction,
    ) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut formats: Vec<_> = {
            match self.supported_configs(stream_t) {
                Err(SupportedStreamConfigsError::DeviceNotAvailable) => {
                    return Err(DefaultStreamConfigError::DeviceNotAvailable);
                }
                Err(SupportedStreamConfigsError::InvalidArgument) => {
                    // this happens sometimes when querying for input and output capabilities but
                    // the device supports only one
                    return Err(DefaultStreamConfigError::StreamTypeNotSupported);
                }
                Err(SupportedStreamConfigsError::BackendSpecific { err }) => {
                    return Err(err.into());
                }
                Ok(fmts) => fmts.collect(),
            }
        };

        formats.sort_by(|a, b| a.cmp_default_heuristics(b));

        match formats.into_iter().last() {
            Some(f) => {
                let min_r = f.min_sample_rate;
                let max_r = f.max_sample_rate;
                let mut format = f.with_max_sample_rate();
                const HZ_44100: SampleRate = SampleRate(44_100);
                if min_r <= HZ_44100 && HZ_44100 <= max_r {
                    format.sample_rate = HZ_44100;
                }
                Ok(format)
            }
            None => Err(DefaultStreamConfigError::StreamTypeNotSupported),
        }
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(alsa::Direction::Capture)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(alsa::Direction::Playback)
    }
}

struct StreamClock {
    sample_rate: SampleRate,

    // In the case that the device does not return valid timestamps via `get_htstamp`, this field
    // will be `Some` and will contain an `Instant` representing the moment the stream was created.
    //
    // If this field is `Some`, then the stream will use the duration since this instant as a
    // source for timestamps.
    //
    // If this field is `None` then the elapsed duration between `get_trigger_htstamp` and
    // `get_htstamp` is used.
    creation_instant: Option<std::time::Instant>,
}

impl StreamClock {
    fn new(status: &alsa::pcm::Status, sample_rate: SampleRate) -> Self {
        // Check to see if we can retrieve valid timestamps from the device.
        // Related: https://bugs.freedesktop.org/show_bug.cgi?id=88503
        let creation_instant = if let libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        } = status.get_htstamp()
        {
            Some(std::time::Instant::now())
        } else {
            None
        };

        StreamClock {
            sample_rate,
            creation_instant,
        }
    }

    fn delay_time(&self, status: &alsa::pcm::Status) -> i64 {
        let delay_frames = status.get_delay() as i64;
        1_000_000_000 * delay_frames / (self.sample_rate.0 as i64)
    }

    fn output_timestamp(&self, status: &alsa::pcm::Status) -> crate::OutputStreamTimestamp {
        match self.creation_instant {
            None => {
                let now = timespec_to_nanos(status.get_htstamp());
                let trigger = timespec_to_nanos(status.get_trigger_htstamp());
                let callback =
                    crate::StreamInstant::from_nanos(if now == 0 { trigger } else { now });

                let audio = if trigger == 0 {
                    now
                } else {
                    trigger + timespec_to_nanos(status.get_audio_htstamp())
                };
                let playback = crate::StreamInstant::from_nanos(audio + self.delay_time(status));

                crate::OutputStreamTimestamp { callback, playback }
            }
            Some(created) => {
                let now = std::time::Instant::now().duration_since(created).as_nanos() as i64;
                let callback = crate::StreamInstant::from_nanos(now);
                let playback = crate::StreamInstant::from_nanos(now + self.delay_time(status));

                crate::OutputStreamTimestamp { callback, playback }
            }
        }
    }

    fn now(&self, status: &alsa::pcm::Status) -> crate::StreamInstant {
        match self.creation_instant {
            None => {
                let now = timespec_to_nanos(status.get_htstamp());
                crate::StreamInstant::from_nanos(if now == 0 {
                    timespec_to_nanos(status.get_trigger_htstamp())
                } else {
                    now
                })
            }
            Some(created) => {
                let now = std::time::Instant::now().duration_since(created).as_nanos() as i64;
                crate::StreamInstant::from_nanos(now)
            }
        }
    }

    fn capture_time(&self, status: &alsa::pcm::Status) -> crate::StreamInstant {
        match self.creation_instant {
            None => {
                let trigger = timespec_to_nanos(status.get_trigger_htstamp());
                let audio = if trigger == 0 {
                    timespec_to_nanos(status.get_htstamp())
                } else {
                    trigger + timespec_to_nanos(status.get_audio_htstamp())
                };
                crate::StreamInstant::from_nanos(audio - self.delay_time(status))
            }
            Some(created) => {
                let now = std::time::Instant::now().duration_since(created).as_nanos() as i64;
                crate::StreamInstant::from_nanos(now - self.delay_time(status))
            }
        }
    }
}

struct StreamInner {
    // The ALSA channel.
    channel: alsa::pcm::PCM,

    // Format of the samples.
    sample_format: SampleFormat,

    // The configuration used to open this stream.
    conf: StreamConfig,

    // Minimum number of samples to put in the buffer.
    period_len: usize,

    #[allow(dead_code)]
    // Whether or not the hardware supports pausing the stream.
    // TODO: We need an API to expose this. See #197, #284.
    can_pause: bool,

    clock: StreamClock,

    dropping: atomic::AtomicBool,
}

// Assume that the ALSA library is built with thread safe option.
unsafe impl Sync for StreamInner {}

pub struct Stream {
    /// The high-priority audio processing thread calling callbacks.
    /// Option used for moving out in destructor.
    thread: Option<JoinHandle<Result<(), ()>>>,

    /// Handle to the underlying stream for playback controls.
    inner: Arc<StreamInner>,
}

fn input_stream_worker(
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) -> Result<(), ()> {
    match stream.sample_format {
        SampleFormat::I16 => {
            let io = stream
                .channel
                .io_i16()
                .map_err(|err| error_callback(err.into()))?;
            input_stream_worker_io(stream, io, data_callback, error_callback)
        }
        SampleFormat::U16 => {
            let io = stream
                .channel
                .io_u16()
                .map_err(|err| error_callback(err.into()))?;
            input_stream_worker_io(stream, io, data_callback, error_callback)
        }
        SampleFormat::F32 => {
            let io = stream
                .channel
                .io_f32()
                .map_err(|err| error_callback(err.into()))?;
            input_stream_worker_io(stream, io, data_callback, error_callback)
        }
    }
}

fn input_stream_worker_io<T: Default + Copy>(
    stream: &StreamInner,
    io: alsa::pcm::IO<'_, T>,
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) -> Result<(), ()> {
    let channels = stream.conf.channels as usize;
    let mut buffer = vec![T::default(); stream.period_len];
    let data = unsafe {
        Data::from_parts(
            buffer.as_mut_ptr() as *mut (),
            buffer.len(),
            stream.sample_format,
        )
    };

    let mut status = stream
        .channel
        .status()
        .map_err(|err| error_callback(err.into()))?;

    while !stream.dropping.load(atomic::Ordering::Relaxed) {
        // Calculate the capture timestamp
        let capture = stream.clock.capture_time(&status);

        // Fill buffer from the stream
        let mut buf = buffer.as_mut_slice();
        while !buf.is_empty() {
            let frames = io
                .readi(buf)
                .or_else(|err| handle_stream_io_error(stream, err, error_callback))?;
            buf = &mut buf[(frames * channels)..];
        }

        // Calculate the callback timestamp
        status = stream
            .channel
            .status()
            .map_err(|err| error_callback(err.into()))?;
        let callback = stream.clock.now(&status);
        let timestamp = crate::InputStreamTimestamp { callback, capture };
        let info = crate::InputCallbackInfo { timestamp };

        // Give data to the callback
        data_callback(&data, &info);
    }

    Ok(())
}

fn output_stream_worker(
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) -> Result<(), ()> {
    match stream.sample_format {
        SampleFormat::I16 => {
            let io = stream
                .channel
                .io_i16()
                .map_err(|err| error_callback(err.into()))?;
            output_stream_worker_io(stream, io, data_callback, error_callback)
        }
        SampleFormat::U16 => {
            let io = stream
                .channel
                .io_u16()
                .map_err(|err| error_callback(err.into()))?;
            output_stream_worker_io(stream, io, data_callback, error_callback)
        }
        SampleFormat::F32 => {
            let io = stream
                .channel
                .io_f32()
                .map_err(|err| error_callback(err.into()))?;
            output_stream_worker_io(stream, io, data_callback, error_callback)
        }
    }
}

fn output_stream_worker_io<T: Default + Copy>(
    stream: &StreamInner,
    io: alsa::pcm::IO<'_, T>,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) -> Result<(), ()> {
    let channels = stream.conf.channels as usize;
    let mut buffer = vec![T::default(); stream.period_len];
    let mut data = unsafe {
        Data::from_parts(
            buffer.as_mut_ptr() as *mut (),
            buffer.len(),
            stream.sample_format,
        )
    };

    while !stream.dropping.load(atomic::Ordering::Relaxed) {
        // Calculate the timestamp
        let status = stream
            .channel
            .status()
            .map_err(|err| error_callback(err.into()))?;
        let timestamp = stream.clock.output_timestamp(&status);
        let info = crate::OutputCallbackInfo { timestamp };

        // Get data from the callback
        data_callback(&mut data, &info);

        // Write the whole buffer to the stream
        let mut buf = &*buffer;
        while !buf.is_empty() {
            let frames = io
                .writei(buf)
                .or_else(|err| handle_stream_io_error(stream, err, error_callback))?;
            buf = &buf[(frames * channels)..];
        }
    }

    Ok(())
}

fn handle_stream_io_error(
    stream: &StreamInner,
    err: alsa::Error,
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) -> Result<usize, ()> {
    if let Some(errno) = err.errno() {
        match errno {
            nix::errno::Errno::EAGAIN => {
                let _ = stream.channel.wait(Some(100));
                Ok(0)
            }
            nix::errno::Errno::EBADFD => match stream.channel.prepare() {
                Ok(()) => Ok(0),
                Err(_) => {
                    error_callback(err.into());
                    Err(())
                }
            },
            nix::errno::Errno::EPIPE | nix::errno::Errno::EINTR | nix::errno::Errno::ESTRPIPE => {
                match stream.channel.try_recover(err, true) {
                    Ok(()) => Ok(0),
                    Err(err) => {
                        error_callback(err.into());
                        Err(())
                    }
                }
            }
            _ => {
                if !stream.dropping.load(atomic::Ordering::Relaxed) {
                    error_callback(err.into());
                }
                Err(())
            }
        }
    } else {
        error_callback(err.into());
        Err(())
    }
}

// Adapted from `timestamp2ns` here:
// https://fossies.org/linux/alsa-lib/test/audio_time.c
fn timespec_to_nanos(ts: libc::timespec) -> i64 {
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

impl Stream {
    fn new_input<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Stream
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Clone the handle for passing into worker thread.
        let stream = inner.clone();
        let thread = thread::Builder::new()
            .name("cpal_alsa_in".to_owned())
            .spawn(move || input_stream_worker(&*stream, &mut data_callback, &mut error_callback))
            .unwrap();
        Stream {
            thread: Some(thread),
            inner,
        }
    }

    fn new_output<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Stream
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Clone the handle for passing into worker thread.
        let stream = inner.clone();
        let thread = thread::Builder::new()
            .name("cpal_alsa_out".to_owned())
            .spawn(move || output_stream_worker(&*stream, &mut data_callback, &mut error_callback))
            .unwrap();
        Stream {
            thread: Some(thread),
            inner,
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.inner.dropping.store(true, atomic::Ordering::Relaxed);
        let _ = self.inner.channel.drop();
        if let Some(thread) = self.thread.take() {
            // Best effort to wait for thread to complete.
            // Ignore errors to avoid panic in drop.
            thread.join().ok();
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.inner.channel.pause(false).ok();
        Ok(())
    }
    fn pause(&self) -> Result<(), PauseStreamError> {
        self.inner.channel.pause(true).ok();
        Ok(())
    }
}

fn set_hw_params_from_format(
    pcm_handle: &alsa::pcm::PCM,
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Result<bool, BackendSpecificError> {
    let hw_params = alsa::pcm::HwParams::any(pcm_handle)?;
    hw_params.set_access(alsa::pcm::Access::RWInterleaved)?;

    let sample_format = if cfg!(target_endian = "big") {
        match sample_format {
            SampleFormat::I16 => alsa::pcm::Format::S16BE,
            SampleFormat::U16 => alsa::pcm::Format::U16BE,
            SampleFormat::F32 => alsa::pcm::Format::FloatBE,
        }
    } else {
        match sample_format {
            SampleFormat::I16 => alsa::pcm::Format::S16LE,
            SampleFormat::U16 => alsa::pcm::Format::U16LE,
            SampleFormat::F32 => alsa::pcm::Format::FloatLE,
        }
    };

    hw_params.set_format(sample_format)?;
    hw_params.set_rate(config.sample_rate.0, alsa::ValueOr::Nearest)?;
    hw_params.set_channels(config.channels as u32)?;

    match config.buffer_size {
        BufferSize::Fixed(v) => {
            hw_params.set_period_size_near((v / 4) as alsa::pcm::Frames, alsa::ValueOr::Nearest)?;
            hw_params.set_buffer_size(v as alsa::pcm::Frames)?;
        }
        BufferSize::Default => {
            // These values together represent a moderate latency and wakeup interval.
            // Without them we are at the mercy of the device
            hw_params.set_period_time_near(25_000, alsa::ValueOr::Nearest)?;
            hw_params.set_buffer_time_near(100_000, alsa::ValueOr::Nearest)?;
        }
    }

    pcm_handle.hw_params(&hw_params)?;

    Ok(hw_params.can_pause())
}

fn set_sw_params_from_format(
    pcm_handle: &alsa::pcm::PCM,
    config: &StreamConfig,
    stream_type: alsa::Direction,
) -> Result<usize, BackendSpecificError> {
    let sw_params = pcm_handle.sw_params_current()?;

    let period_len = {
        let (buffer, period) = pcm_handle.get_params()?;
        if buffer == 0 {
            return Err(BackendSpecificError {
                description: "initialization resulted in a null buffer".to_string(),
            });
        }
        sw_params.set_avail_min(period as alsa::pcm::Frames)?;

        let start_threshold = match stream_type {
            alsa::Direction::Playback => buffer - period,
            alsa::Direction::Capture => 1,
        };
        sw_params.set_start_threshold(start_threshold.try_into().unwrap())?;

        period as usize * config.channels as usize
    };

    sw_params.set_tstamp_mode(true)?;
    sw_params.set_tstamp_type(alsa::pcm::TstampType::MonotonicRaw)?;

    pcm_handle.sw_params(&sw_params)?;

    Ok(period_len)
}

impl From<alsa::Error> for BackendSpecificError {
    fn from(err: alsa::Error) -> Self {
        BackendSpecificError {
            description: err.to_string(),
        }
    }
}

impl From<alsa::Error> for BuildStreamError {
    fn from(err: alsa::Error) -> Self {
        match err.errno() {
            Some(nix::errno::Errno::EBUSY) => BuildStreamError::DeviceNotAvailable,
            Some(nix::errno::Errno::EINVAL) => BuildStreamError::InvalidArgument,
            _ => BackendSpecificError::from(err).into(),
        }
    }
}

impl From<alsa::Error> for SupportedStreamConfigsError {
    fn from(err: alsa::Error) -> Self {
        match err.errno() {
            Some(nix::errno::Errno::ENOENT) | Some(nix::errno::Errno::EBUSY) => {
                SupportedStreamConfigsError::DeviceNotAvailable
            }
            Some(nix::errno::Errno::EINVAL) => SupportedStreamConfigsError::InvalidArgument,
            _ => BackendSpecificError::from(err).into(),
        }
    }
}

impl From<alsa::Error> for PlayStreamError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}

impl From<alsa::Error> for PauseStreamError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}

impl From<alsa::Error> for StreamError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
    }
}
