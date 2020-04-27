extern crate alsa;
extern crate libc;

use self::alsa::poll::Descriptors;
use crate::{
    BackendSpecificError, BuildStreamError, ChannelCount, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError, SupportedStreamConfig,
    SupportedStreamConfigRange, SupportedStreamConfigsError,
};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::vec::IntoIter as VecIntoIter;
use std::{cmp, mem};
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

struct TriggerSender(libc::c_int);

struct TriggerReceiver(libc::c_int);

impl TriggerSender {
    fn wakeup(&self) {
        let buf = 1u64;
        let ret = unsafe { libc::write(self.0, &buf as *const u64 as *const _, 8) };
        assert!(ret == 8);
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device(String);

impl Device {
    fn build_stream_inner(
        &self,
        conf: &StreamConfig,
        sample_format: SampleFormat,
        stream_type: alsa::Direction,
    ) -> Result<StreamInner, BuildStreamError> {
        let name = &self.0;

        let handle = match alsa::pcm::PCM::new(name, stream_type, true).map_err(|e| (e, e.errno()))
        {
            Err((_, Some(nix::errno::Errno::EBUSY))) => {
                return Err(BuildStreamError::DeviceNotAvailable)
            }
            Err((_, Some(nix::errno::Errno::EINVAL))) => {
                return Err(BuildStreamError::InvalidArgument)
            }
            Err((e, _)) => return Err(e.into()),
            Ok(handle) => handle,
        };
        let can_pause = {
            let hw_params = set_hw_params_from_format(&handle, conf, sample_format)?;
            hw_params.can_pause()
        };
        let (buffer_len, period_len) = set_sw_params_from_format(&handle, conf)?;

        handle.prepare()?;

        let num_descriptors = {
            let num_descriptors = handle.count();
            if num_descriptors == 0 {
                let description = "poll descriptor count for stream was 0".to_string();
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            num_descriptors
        };

        handle.start()?;

        let stream_inner = StreamInner {
            channel: handle,
            sample_format,
            num_descriptors,
            num_channels: conf.channels as u16,
            buffer_len,
            period_len,
            can_pause,
        };

        Ok(stream_inner)
    }

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.0.clone())
    }

    fn supported_configs(
        &self,
        stream_t: alsa::Direction,
    ) -> Result<VecIntoIter<SupportedStreamConfigRange>, SupportedStreamConfigsError> {
        let name = &self.0;

        let handle = match alsa::pcm::PCM::new(name, stream_t, true).map_err(|e| (e, e.errno())) {
            Err((_, Some(nix::errno::Errno::ENOENT)))
            | Err((_, Some(nix::errno::Errno::EBUSY))) => {
                return Err(SupportedStreamConfigsError::DeviceNotAvailable)
            }
            Err((_, Some(nix::errno::Errno::EINVAL))) => {
                return Err(SupportedStreamConfigsError::InvalidArgument)
            }
            Err((e, _)) => return Err(e.into()),
            Ok(handle) => handle,
        };

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

            if rates.len() == 0 {
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

        let mut output = Vec::with_capacity(
            supported_formats.len() * supported_channels.len() * sample_rates.len(),
        );
        for &sample_format in supported_formats.iter() {
            for channels in supported_channels.iter() {
                for &(min_rate, max_rate) in sample_rates.iter() {
                    output.push(SupportedStreamConfigRange {
                        channels: channels.clone(),
                        min_sample_rate: SampleRate(min_rate as u32),
                        max_sample_rate: SampleRate(max_rate as u32),
                        sample_format: sample_format,
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

struct StreamInner {
    // The ALSA channel.
    channel: alsa::pcm::PCM,

    // When converting between file descriptors and `snd_pcm_t`, this is the number of
    // file descriptors that this `snd_pcm_t` uses.
    num_descriptors: usize,

    // Format of the samples.
    sample_format: SampleFormat,

    // Number of channels, ie. number of samples per frame.
    num_channels: u16,

    // Number of samples that can fit in the buffer.
    buffer_len: usize,

    // Minimum number of samples to put in the buffer.
    period_len: usize,

    // Whether or not the hardware supports pausing the stream.
    can_pause: bool,
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

#[derive(Default)]
struct StreamWorkerContext {
    descriptors: Vec<libc::pollfd>,
    buffer: Vec<u8>,
}

fn input_stream_worker(
    rx: TriggerReceiver,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) {
    let mut ctxt = StreamWorkerContext::default();
    loop {
        let flow = report_error(
            poll_descriptors_and_prepare_buffer(&rx, stream, &mut ctxt),
            error_callback,
        )
        .unwrap_or(PollDescriptorsFlow::Continue);

        match flow {
            PollDescriptorsFlow::Continue => continue,
            PollDescriptorsFlow::Return => return,
            PollDescriptorsFlow::Ready {
                available_frames: _,
                stream_type,
            } => {
                assert_eq!(
                    stream_type,
                    StreamType::Input,
                    "expected input stream, but polling descriptors indicated output",
                );
                report_error(
                    process_input(stream, &mut ctxt.buffer, data_callback),
                    error_callback,
                );
            }
        }
    }
}

fn output_stream_worker(
    rx: TriggerReceiver,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) {
    let mut ctxt = StreamWorkerContext::default();
    loop {
        let flow = report_error(
            poll_descriptors_and_prepare_buffer(&rx, stream, &mut ctxt),
            error_callback,
        )
        .unwrap_or(PollDescriptorsFlow::Continue);

        match flow {
            PollDescriptorsFlow::Continue => continue,
            PollDescriptorsFlow::Return => return,
            PollDescriptorsFlow::Ready {
                available_frames,
                stream_type,
            } => {
                assert_eq!(
                    stream_type,
                    StreamType::Output,
                    "expected output stream, but polling descriptors indicated input",
                );
                process_output(
                    stream,
                    &mut ctxt.buffer,
                    available_frames,
                    data_callback,
                    error_callback,
                );
            }
        }
    }
}

fn report_error<T, E>(
    result: Result<T, E>,
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) -> Option<T>
where
    E: Into<StreamError>,
{
    match result {
        Ok(val) => Some(val),
        Err(err) => {
            error_callback(err.into());
            None
        }
    }
}

enum PollDescriptorsFlow {
    Continue,
    Return,
    Ready {
        stream_type: StreamType,
        available_frames: usize,
    },
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
    let res = alsa::poll::poll(descriptors, -1)?;
    if res == 0 {
        let description = String::from("`alsa::poll()` spuriously returned");
        return Err(BackendSpecificError { description });
    }

    if descriptors[0].revents != 0 {
        // The stream has been requested to be destroyed.
        rx.clear_pipe();
        return Ok(PollDescriptorsFlow::Return);
    }

    let stream_type = match stream.channel.revents(&descriptors[1..])? {
        alsa::poll::Flags::OUT => StreamType::Output,
        alsa::poll::Flags::IN => StreamType::Input,
        _ => {
            // Nothing to process, poll again
            return Ok(PollDescriptorsFlow::Continue);
        }
    };
    // Get the number of available samples for reading/writing.
    let available_samples = get_available_samples(stream)?;

    // Only go on if there is at least `stream.period_len` samples.
    if available_samples < stream.period_len {
        return Ok(PollDescriptorsFlow::Continue);
    }

    // Prepare the data buffer.
    let buffer_size = stream.sample_format.sample_size() * available_samples;
    buffer.resize(buffer_size, 0u8);
    let available_frames = available_samples / stream.num_channels as usize;

    Ok(PollDescriptorsFlow::Ready {
        stream_type,
        available_frames,
    })
}

// Read input data from ALSA and deliver it to the user.
fn process_input(
    stream: &StreamInner,
    buffer: &mut [u8],
    data_callback: &mut (dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static),
) -> Result<(), BackendSpecificError> {
    stream.channel.io().readi(buffer)?;
    let sample_format = stream.sample_format;
    let data = buffer.as_mut_ptr() as *mut ();
    let len = buffer.len() / sample_format.sample_size();
    let data = unsafe { Data::from_parts(data, len, sample_format) };
    let info = crate::InputCallbackInfo {};
    data_callback(&data, &info);

    Ok(())
}

// Request data from the user's function and write it via ALSA.
//
// Returns `true`
fn process_output(
    stream: &StreamInner,
    buffer: &mut [u8],
    available_frames: usize,
    data_callback: &mut (dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static),
    error_callback: &mut dyn FnMut(StreamError),
) {
    {
        // We're now sure that we're ready to write data.
        let sample_format = stream.sample_format;
        let data = buffer.as_mut_ptr() as *mut ();
        let len = buffer.len() / sample_format.sample_size();
        let mut data = unsafe { Data::from_parts(data, len, sample_format) };
        let info = crate::OutputCallbackInfo {};
        data_callback(&mut data, &info);
    }
    loop {
        match stream.channel.io().writei(buffer) {
            Err(err) if err.errno() == Some(nix::errno::Errno::EPIPE) => {
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
        let (tx, rx) = trigger();
        // Clone the handle for passing into worker thread.
        let stream = inner.clone();
        let thread = thread::spawn(move || {
            input_stream_worker(rx, &*stream, &mut data_callback, &mut error_callback);
        });
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
    ) -> Stream
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let (tx, rx) = trigger();
        // Clone the handle for passing into worker thread.
        let stream = inner.clone();
        let thread = thread::spawn(move || {
            output_stream_worker(rx, &*stream, &mut data_callback, &mut error_callback);
        });
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

// Determine the number of samples that are available to read/write.
fn get_available_samples(stream: &StreamInner) -> Result<usize, BackendSpecificError> {
    match stream.channel.avail_update() {
        Err(err) if err.errno() == Some(nix::errno::Errno::EPIPE) => {
            // buffer underrun
            // TODO: Notify the user some how.
            Ok(stream.buffer_len)
        }
        Err(err) => Err(err.into()),
        Ok(available) => Ok(available as usize * stream.num_channels as usize),
    }
}

fn set_hw_params_from_format<'a>(
    pcm_handle: &'a alsa::pcm::PCM,
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Result<alsa::pcm::HwParams<'a>, BackendSpecificError> {
    let mut hw_params = alsa::pcm::HwParams::any(pcm_handle)?;
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

    // If this isn't set manually a overlarge buffer may be used causing audio delay
    let mut hw_params_copy = hw_params.clone();
    if let Err(_) = hw_params.set_buffer_time_near(100_000, alsa::ValueOr::Nearest) {
        // Swap out the params with errors for a snapshot taken before the error was introduced.
        mem::swap(&mut hw_params_copy, &mut hw_params);
    }

    pcm_handle.hw_params(&hw_params)?;

    Ok(hw_params)
}

fn set_sw_params_from_format(
    pcm_handle: &alsa::pcm::PCM,
    config: &StreamConfig,
) -> Result<(usize, usize), BackendSpecificError> {
    let sw_params = pcm_handle.sw_params_current()?;
    sw_params.set_start_threshold(0)?;

    let (buffer_len, period_len) = {
        let (buffer, period) = pcm_handle.get_params()?;
        if buffer == 0 {
            return Err(BackendSpecificError {
                description: "initialization resulted in a null buffer".to_string(),
            });
        }
        sw_params.set_avail_min(period as alsa::pcm::Frames)?;
        let buffer = buffer as usize * config.channels as usize;
        let period = period as usize * config.channels as usize;
        (buffer, period)
    };

    pcm_handle.sw_params(&sw_params)?;

    Ok((buffer_len, period_len))
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
