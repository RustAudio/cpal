extern crate alsa;
extern crate libc;

use self::alsa::poll::Descriptors;
use crate::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, ChannelCount, Data,
    DefaultStreamConfigError, DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo,
    PauseStreamError, PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use std::cmp;
use std::convert::TryInto;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::vec::IntoIter as VecIntoIter;

pub use self::enumerate::{default_input_device, default_output_device, Devices};

pub type SupportedInputConfigs = VecIntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = VecIntoIter<SupportedStreamConfigRange>;

mod enumerate;

/// The default linux, dragonfly, freebsd and netbsd host type.
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
        // Assume ALSA is always available on linux/dragonfly/freebsd/netbsd.
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

    fn build_input_stream_raw<D, E>(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Capture)?;
        let stream = Stream::new_input(
            Arc::new(stream_inner),
            data_callback,
            error_callback,
            timeout,
        );
        Ok(stream)
    }

    fn build_output_stream_raw<D, E>(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner =
            self.build_stream_inner(conf, sample_format, alsa::Direction::Playback)?;
        let stream = Stream::new_output(
            Arc::new(stream_inner),
            data_callback,
            error_callback,
            timeout,
        );
        Ok(stream)
    }
}

struct TriggerSender(libc::c_int);

struct TriggerReceiver(libc::c_int);

impl TriggerSender {
    fn wakeup(&self) {
        let buf = 1u64;
        let ret = unsafe { libc::write(self.0, &buf as *const u64 as *const _, 8) };
        assert_eq!(ret, 8);
    }
}

impl TriggerReceiver {
    fn clear_pipe(&self) {
        let mut out = 0u64;
        let ret = unsafe { libc::read(self.0, &mut out as *mut u64 as *mut _, 8) };
        assert_eq!(ret, 8);
    }
}

fn trigger() -> (TriggerSender, TriggerReceiver) {
    let mut fds = [0, 0];
    match unsafe { libc::pipe(fds.as_mut_ptr()) } {
        0 => (TriggerSender(fds[1]), TriggerReceiver(fds[0])),
        _ => panic!("Could not create pipe"),
    }
}

impl Drop for TriggerSender {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

impl Drop for TriggerReceiver {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

#[derive(Default)]
struct DeviceHandles {
    playback: Option<alsa::PCM>,
    capture: Option<alsa::PCM>,
}

impl DeviceHandles {
    /// Create `DeviceHandles` for `name` and try to open a handle for both
    /// directions. Returns `Ok` if either direction is opened successfully.
    fn open(name: &str) -> Result<Self, alsa::Error> {
        let mut handles = Self::default();
        let playback_err = handles.try_open(name, alsa::Direction::Playback).err();
        let capture_err = handles.try_open(name, alsa::Direction::Capture).err();
        if let Some(err) = capture_err.and(playback_err) {
            Err(err)
        } else {
            Ok(handles)
        }
    }

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

#[derive(Clone)]
pub struct Device {
    name: String,
    handles: Arc<Mutex<DeviceHandles>>,
}

impl Device {
    fn build_stream_inner(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        stream_type: alsa::Direction,
    ) -> Result<StreamInner, BuildStreamError> {
        let handle_result = self
            .handles
            .lock()
            .unwrap()
            .take(&self.name, stream_type)
            .map_err(|e| (e, e.errno()));

        let handle = match handle_result {
            Err((_, libc::EBUSY)) => return Err(BuildStreamError::DeviceNotAvailable),
            Err((_, libc::EINVAL)) => return Err(BuildStreamError::InvalidArgument),
            Err((e, _)) => return Err(e.into()),
            Ok(handle) => handle,
        };
        let can_pause = set_hw_params_from_format(&handle, conf, sample_format)?;
        let period_len = set_sw_params_from_format(&handle, conf, stream_type)?;

        handle.prepare()?;

        let num_descriptors = handle.count();
        if num_descriptors == 0 {
            let description = "poll descriptor count for stream was 0".to_string();
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        // Check to see if we can retrieve valid timestamps from the device.
        // Related: https://bugs.freedesktop.org/show_bug.cgi?id=88503
        let ts = handle.status()?.get_htstamp();
        let creation_instant = match (ts.tv_sec, ts.tv_nsec) {
            (0, 0) => Some(std::time::Instant::now()),
            _ => None,
        };

        if let alsa::Direction::Capture = stream_type {
            handle.start()?;
        }

        let stream_inner = StreamInner {
            channel: handle,
            sample_format,
            num_descriptors,
            conf: conf.clone(),
            period_len,
            can_pause,
            creation_instant,
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
        let mut guard = self.handles.lock().unwrap();
        let handle_result = guard
            .get_mut(&self.name, stream_t)
            .map_err(|e| (e, e.errno()));

        let handle = match handle_result {
            Err((_, libc::ENOENT)) | Err((_, libc::EBUSY)) => {
                return Err(SupportedStreamConfigsError::DeviceNotAvailable)
            }
            Err((_, libc::EINVAL)) => return Err(SupportedStreamConfigsError::InvalidArgument),
            Err((e, _)) => return Err(e.into()),
            Ok(handle) => handle,
        };

        let hw_params = alsa::pcm::HwParams::any(handle)?;

        // TODO: check endianness
        const FORMATS: [(SampleFormat, alsa::pcm::Format); 8] = [
            (SampleFormat::I8, alsa::pcm::Format::S8),
            (SampleFormat::U8, alsa::pcm::Format::U8),
            (SampleFormat::I16, alsa::pcm::Format::S16LE),
            //SND_PCM_FORMAT_S16_BE,
            (SampleFormat::U16, alsa::pcm::Format::U16LE),
            //SND_PCM_FORMAT_U16_BE,
            //SND_PCM_FORMAT_S24_LE,
            //SND_PCM_FORMAT_S24_BE,
            //SND_PCM_FORMAT_U24_LE,
            //SND_PCM_FORMAT_U24_BE,
            (SampleFormat::I32, alsa::pcm::Format::S32LE),
            //SND_PCM_FORMAT_S32_BE,
            (SampleFormat::U32, alsa::pcm::Format::U32LE),
            //SND_PCM_FORMAT_U32_BE,
            (SampleFormat::F32, alsa::pcm::Format::FloatLE),
            //SND_PCM_FORMAT_FLOAT_BE,
            (SampleFormat::F64, alsa::pcm::Format::Float64LE),
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
                    // this happens sometimes when querying for input and output capabilities, but
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

struct StreamInner {
    // The ALSA channel.
    channel: alsa::pcm::PCM,

    // When converting between file descriptors and `snd_pcm_t`, this is the number of
    // file descriptors that this `snd_pcm_t` uses.
    num_descriptors: usize,

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

// Assume that the ALSA library is built with thread safe option.
unsafe impl Sync for StreamInner {}

#[derive(Debug, Eq, PartialEq)]
enum StreamType {
    Input,
    Output,
}

pub struct Stream {
    /// The high-priority audio processing thread calling callbacks.
    /// Option used for moving out in destructor.
    thread: Option<JoinHandle<()>>,

    /// Handle to the underlying stream for playback controls.
    inner: Arc<StreamInner>,

    /// Used to signal to stop processing.
    trigger: TriggerSender,
}

struct StreamWorkerContext {
    descriptors: Vec<libc::pollfd>,
    buffer: Vec<u8>,
    poll_timeout: i32,
}

impl StreamWorkerContext {
    fn new(poll_timeout: &Option<Duration>) -> Self {
        let poll_timeout: i32 = if let Some(d) = poll_timeout {
            d.as_millis().try_into().unwrap()
        } else {
            -1
        };

        Self {
            descriptors: Vec::new(),
            buffer: Vec::new(),
            poll_timeout,
        }
    }
}

fn input_stream_worker(
    rx: TriggerReceiver,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
    timeout: Option<Duration>,
) {
    let mut ctxt = StreamWorkerContext::new(&timeout);
    loop {
        let flow =
            poll_descriptors_and_prepare_buffer(&rx, stream, &mut ctxt).unwrap_or_else(|err| {
                error_callback(err.into());
                PollDescriptorsFlow::Continue
            });

        match flow {
            PollDescriptorsFlow::Continue => {
                continue;
            }
            PollDescriptorsFlow::XRun => {
                if let Err(err) = stream.channel.prepare() {
                    error_callback(err.into());
                }
                continue;
            }
            PollDescriptorsFlow::Return => return,
            PollDescriptorsFlow::Ready {
                status,
                avail_frames: _,
                delay_frames,
                stream_type,
            } => {
                assert_eq!(
                    stream_type,
                    StreamType::Input,
                    "expected input stream, but polling descriptors indicated output",
                );
                if let Err(err) = process_input(
                    stream,
                    &mut ctxt.buffer,
                    status,
                    delay_frames,
                    data_callback,
                ) {
                    error_callback(err.into());
                }
            }
        }
    }
}

fn output_stream_worker(
    rx: TriggerReceiver,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
    timeout: Option<Duration>,
) {
    let mut ctxt = StreamWorkerContext::new(&timeout);
    loop {
        let flow =
            poll_descriptors_and_prepare_buffer(&rx, stream, &mut ctxt).unwrap_or_else(|err| {
                error_callback(err.into());
                PollDescriptorsFlow::Continue
            });

        match flow {
            PollDescriptorsFlow::Continue => continue,
            PollDescriptorsFlow::XRun => {
                if let Err(err) = stream.channel.prepare() {
                    error_callback(err.into());
                }
                continue;
            }
            PollDescriptorsFlow::Return => return,
            PollDescriptorsFlow::Ready {
                status,
                avail_frames,
                delay_frames,
                stream_type,
            } => {
                assert_eq!(
                    stream_type,
                    StreamType::Output,
                    "expected output stream, but polling descriptors indicated input",
                );
                if let Err(err) = process_output(
                    stream,
                    &mut ctxt.buffer,
                    status,
                    avail_frames,
                    delay_frames,
                    data_callback,
                    error_callback,
                ) {
                    error_callback(err.into());
                }
            }
        }
    }
}

enum PollDescriptorsFlow {
    Continue,
    Return,
    Ready {
        stream_type: StreamType,
        status: alsa::pcm::Status,
        avail_frames: usize,
        delay_frames: usize,
    },
    XRun,
}

// This block is shared between both input and output stream worker functions.
fn poll_descriptors_and_prepare_buffer(
    rx: &TriggerReceiver,
    stream: &StreamInner,
    ctxt: &mut StreamWorkerContext,
) -> Result<PollDescriptorsFlow, BackendSpecificError> {
    let StreamWorkerContext {
        ref mut descriptors,
        ref mut buffer,
        ref poll_timeout,
    } = *ctxt;

    descriptors.clear();

    // Add the self-pipe for signaling termination.
    descriptors.push(libc::pollfd {
        fd: rx.0,
        events: libc::POLLIN,
        revents: 0,
    });

    // Add ALSA polling fds.
    let len = descriptors.len();
    descriptors.resize(
        stream.num_descriptors + len,
        libc::pollfd {
            fd: 0,
            events: 0,
            revents: 0,
        },
    );
    let filled = stream.channel.fill(&mut descriptors[len..])?;
    debug_assert_eq!(filled, stream.num_descriptors);

    // Don't timeout, wait forever.
    let res = alsa::poll::poll(descriptors, *poll_timeout)?;
    if res == 0 {
        let description = String::from("`alsa::poll()` spuriously returned");
        return Err(BackendSpecificError { description });
    }

    if descriptors[0].revents != 0 {
        // The stream has been requested to be destroyed.
        rx.clear_pipe();
        return Ok(PollDescriptorsFlow::Return);
    }

    let revents = stream.channel.revents(&descriptors[1..])?;
    if revents.contains(alsa::poll::Flags::ERR) {
        let description = String::from("`alsa::poll()` returned POLLERR");
        return Err(BackendSpecificError { description });
    }
    let stream_type = match revents {
        alsa::poll::Flags::OUT => StreamType::Output,
        alsa::poll::Flags::IN => StreamType::Input,
        _ => {
            // Nothing to process, poll again
            return Ok(PollDescriptorsFlow::Continue);
        }
    };

    let status = stream.channel.status()?;
    let avail_frames = match stream.channel.avail() {
        Err(err) if err.errno() == libc::EPIPE => return Ok(PollDescriptorsFlow::XRun),
        res => res,
    }? as usize;
    let delay_frames = match status.get_delay() {
        // Buffer underrun. TODO: Notify the user.
        d if d < 0 => 0,
        d => d as usize,
    };
    let available_samples = avail_frames * stream.conf.channels as usize;

    // Only go on if there is at least `stream.period_len` samples.
    if available_samples < stream.period_len {
        return Ok(PollDescriptorsFlow::Continue);
    }

    // Prepare the data buffer.
    let buffer_size = stream.sample_format.sample_size() * available_samples;
    buffer.resize(buffer_size, 0u8);

    Ok(PollDescriptorsFlow::Ready {
        stream_type,
        status,
        avail_frames,
        delay_frames,
    })
}

// Read input data from ALSA and deliver it to the user.
fn process_input(
    stream: &StreamInner,
    buffer: &mut [u8],
    status: alsa::pcm::Status,
    delay_frames: usize,
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
) -> Result<(), BackendSpecificError> {
    stream.channel.io_bytes().readi(buffer)?;
    let sample_format = stream.sample_format;
    let data = buffer.as_mut_ptr() as *mut ();
    let len = buffer.len() / sample_format.sample_size();
    let data = unsafe { Data::from_parts(data, len, sample_format) };
    let callback = stream_timestamp(&status, stream.creation_instant)?;
    let delay_duration = frames_to_duration(delay_frames, stream.conf.sample_rate);
    let capture = callback
        .sub(delay_duration)
        .expect("`capture` is earlier than representation supported by `StreamInstant`");
    let timestamp = crate::InputStreamTimestamp { callback, capture };
    let info = crate::InputCallbackInfo { timestamp };
    data_callback(&data, &info);

    Ok(())
}

// Request data from the user's function and write it via ALSA.
//
// Returns `true`
fn process_output(
    stream: &StreamInner,
    buffer: &mut [u8],
    status: alsa::pcm::Status,
    available_frames: usize,
    delay_frames: usize,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
    error_callback: &mut dyn FnMut(StreamError),
) -> Result<(), BackendSpecificError> {
    {
        // We're now sure that we're ready to write data.
        let sample_format = stream.sample_format;
        let data = buffer.as_mut_ptr() as *mut ();
        let len = buffer.len() / sample_format.sample_size();
        let mut data = unsafe { Data::from_parts(data, len, sample_format) };
        let callback = stream_timestamp(&status, stream.creation_instant)?;
        let delay_duration = frames_to_duration(delay_frames, stream.conf.sample_rate);
        let playback = callback
            .add(delay_duration)
            .expect("`playback` occurs beyond representation supported by `StreamInstant`");
        let timestamp = crate::OutputStreamTimestamp { callback, playback };
        let info = crate::OutputCallbackInfo { timestamp };
        data_callback(&mut data, &info);
    }
    loop {
        match stream.channel.io_bytes().writei(buffer) {
            Err(err) if err.errno() == libc::EPIPE => {
                // buffer underrun
                // TODO: Notify the user of this.
                let _ = stream.channel.try_recover(err, false);
            }
            Err(err) => {
                error_callback(err.into());
                continue;
            }
            Ok(result) if result != available_frames => {
                let description = format!(
                    "unexpected number of frames written: expected {}, \
                     result {} (this should never happen)",
                    available_frames, result,
                );
                error_callback(BackendSpecificError { description }.into());
                continue;
            }
            _ => {
                break;
            }
        }
    }
    Ok(())
}

// Use the elapsed duration since the start of the stream.
//
// This ensures positive values that are compatible with our `StreamInstant` representation.
fn stream_timestamp(
    status: &alsa::pcm::Status,
    creation_instant: Option<std::time::Instant>,
) -> Result<crate::StreamInstant, BackendSpecificError> {
    match creation_instant {
        None => {
            let trigger_ts = status.get_trigger_htstamp();
            let ts = status.get_htstamp();
            let nanos = timespec_diff_nanos(ts, trigger_ts);
            if nanos < 0 {
                panic!(
                    "get_htstamp `{}.{}` was earlier than get_trigger_htstamp `{}.{}`",
                    ts.tv_sec, ts.tv_nsec, trigger_ts.tv_sec, trigger_ts.tv_nsec
                );
            }
            Ok(crate::StreamInstant::from_nanos(nanos))
        }
        Some(creation) => {
            let now = std::time::Instant::now();
            let duration = now.duration_since(creation);
            let instant = crate::StreamInstant::from_nanos_i128(duration.as_nanos() as i128)
                .expect("stream duration has exceeded `StreamInstant` representation");
            Ok(instant)
        }
    }
}

// Adapted from `timestamp2ns` here:
// https://fossies.org/linux/alsa-lib/test/audio_time.c
fn timespec_to_nanos(ts: libc::timespec) -> i64 {
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

// Adapted from `timediff` here:
// https://fossies.org/linux/alsa-lib/test/audio_time.c
fn timespec_diff_nanos(a: libc::timespec, b: libc::timespec) -> i64 {
    timespec_to_nanos(a) - timespec_to_nanos(b)
}

// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: usize, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

impl Stream {
    fn new_input<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
        timeout: Option<Duration>,
    ) -> Stream
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let (tx, rx) = trigger();
        // Clone the handle for passing into worker thread.
        let stream = inner.clone();
        let thread = thread::Builder::new()
            .name("cpal_alsa_in".to_owned())
            .spawn(move || {
                input_stream_worker(
                    rx,
                    &stream,
                    &mut data_callback,
                    &mut error_callback,
                    timeout,
                );
            })
            .unwrap();
        Stream {
            thread: Some(thread),
            inner,
            trigger: tx,
        }
    }

    fn new_output<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
        timeout: Option<Duration>,
    ) -> Stream
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let (tx, rx) = trigger();
        // Clone the handle for passing into worker thread.
        let stream = inner.clone();
        let thread = thread::Builder::new()
            .name("cpal_alsa_out".to_owned())
            .spawn(move || {
                output_stream_worker(
                    rx,
                    &stream,
                    &mut data_callback,
                    &mut error_callback,
                    timeout,
                );
            })
            .unwrap();
        Stream {
            thread: Some(thread),
            inner,
            trigger: tx,
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.trigger.wakeup();
        self.thread.take().unwrap().join().unwrap();
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
            SampleFormat::I8 => alsa::pcm::Format::S8,
            SampleFormat::I16 => alsa::pcm::Format::S16BE,
            // SampleFormat::I24 => alsa::pcm::Format::S24BE,
            SampleFormat::I32 => alsa::pcm::Format::S32BE,
            // SampleFormat::I48 => alsa::pcm::Format::S48BE,
            // SampleFormat::I64 => alsa::pcm::Format::S64BE,
            SampleFormat::U8 => alsa::pcm::Format::U8,
            SampleFormat::U16 => alsa::pcm::Format::U16BE,
            // SampleFormat::U24 => alsa::pcm::Format::U24BE,
            SampleFormat::U32 => alsa::pcm::Format::U32BE,
            // SampleFormat::U48 => alsa::pcm::Format::U48BE,
            // SampleFormat::U64 => alsa::pcm::Format::U64BE,
            SampleFormat::F32 => alsa::pcm::Format::FloatBE,
            SampleFormat::F64 => alsa::pcm::Format::Float64BE,
            sample_format => {
                return Err(BackendSpecificError {
                    description: format!(
                        "Sample format '{}' is not supported by this backend",
                        sample_format
                    ),
                })
            }
        }
    } else {
        match sample_format {
            SampleFormat::I8 => alsa::pcm::Format::S8,
            SampleFormat::I16 => alsa::pcm::Format::S16LE,
            // SampleFormat::I24 => alsa::pcm::Format::S24LE,
            SampleFormat::I32 => alsa::pcm::Format::S32LE,
            // SampleFormat::I48 => alsa::pcm::Format::S48LE,
            // SampleFormat::I64 => alsa::pcm::Format::S64LE,
            SampleFormat::U8 => alsa::pcm::Format::U8,
            SampleFormat::U16 => alsa::pcm::Format::U16LE,
            // SampleFormat::U24 => alsa::pcm::Format::U24LE,
            SampleFormat::U32 => alsa::pcm::Format::U32LE,
            // SampleFormat::U48 => alsa::pcm::Format::U48LE,
            // SampleFormat::U64 => alsa::pcm::Format::U64LE,
            SampleFormat::F32 => alsa::pcm::Format::FloatLE,
            SampleFormat::F64 => alsa::pcm::Format::Float64LE,
            sample_format => {
                return Err(BackendSpecificError {
                    description: format!(
                        "Sample format '{}' is not supported by this backend",
                        sample_format
                    ),
                })
            }
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
            // Without them, we are at the mercy of the device
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

            // For capture streams, the start threshold is irrelevant and ignored,
            // because build_stream_inner() starts the stream before process_input()
            // reads from it. Set it anyway I guess, since it's better than leaving
            // it at an unspecified default value.
            alsa::Direction::Capture => 1,
        };
        sw_params.set_start_threshold(start_threshold.try_into().unwrap())?;

        period as usize * config.channels as usize
    };

    sw_params.set_tstamp_mode(true)?;
    sw_params.set_tstamp_type(alsa::pcm::TstampType::MonotonicRaw)?;

    // tstamp_type param cannot be changed after the device is opened.
    // The default tstamp_type value on most Linux systems is "monotonic",
    // let's try to use it if setting the tstamp_type fails.
    if pcm_handle.sw_params(&sw_params).is_err() {
        sw_params.set_tstamp_type(alsa::pcm::TstampType::Monotonic)?;
        pcm_handle.sw_params(&sw_params)?;
    }

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
        let err: BackendSpecificError = err.into();
        err.into()
    }
}

impl From<alsa::Error> for SupportedStreamConfigsError {
    fn from(err: alsa::Error) -> Self {
        let err: BackendSpecificError = err.into();
        err.into()
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
