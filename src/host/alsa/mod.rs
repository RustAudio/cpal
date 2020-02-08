extern crate alsa_sys as alsa;

use crate::{
    BackendSpecificError, BuildStreamError, ChannelCount, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, PauseStreamError, PlayStreamError, SampleFormat, SampleRate,
    StreamConfig, StreamError, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};
use std::os::raw;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::vec::IntoIter as VecIntoIter;
use std::{cmp, ffi, io, ptr};
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
        D: FnMut(&Data) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner = self.build_stream_inner(
            conf,
            sample_format,
            alsa::_snd_pcm_stream::SND_PCM_STREAM_CAPTURE,
        )?;
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
        D: FnMut(&mut Data) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner = self.build_stream_inner(
            conf,
            sample_format,
            alsa::_snd_pcm_stream::SND_PCM_STREAM_PLAYBACK,
        )?;
        let stream = Stream::new_output(Arc::new(stream_inner), data_callback, error_callback);
        Ok(stream)
    }
}

struct TriggerSender(raw::c_int);

struct TriggerReceiver(raw::c_int);

impl TriggerSender {
    fn wakeup(&self) {
        let buf = 1u64;
        let ret = unsafe { alsa::write(self.0, &buf as *const u64 as *const _, 8) };
        assert!(ret == 8);
    }
}

impl TriggerReceiver {
    fn clear_pipe(&self) {
        let mut out = 0u64;
        let ret = unsafe { alsa::read(self.0, &mut out as *mut u64 as *mut _, 8) };
        assert_eq!(ret, 8);
    }
}

fn trigger() -> (TriggerSender, TriggerReceiver) {
    let mut fds = [0, 0];
    match unsafe { alsa::pipe(fds.as_mut_ptr()) } {
        0 => (TriggerSender(fds[1]), TriggerReceiver(fds[0])),
        _ => panic!("Could not create pipe"),
    }
}

impl Drop for TriggerSender {
    fn drop(&mut self) {
        unsafe {
            alsa::close(self.0);
        }
    }
}

impl Drop for TriggerReceiver {
    fn drop(&mut self) {
        unsafe {
            alsa::close(self.0);
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
        stream_type: alsa::snd_pcm_stream_t,
    ) -> Result<StreamInner, BuildStreamError> {
        let name = ffi::CString::new(self.0.clone()).expect("unable to clone device");

        let handle = unsafe {
            let mut handle = ptr::null_mut();
            match alsa::snd_pcm_open(
                &mut handle,
                name.as_ptr(),
                stream_type,
                alsa::SND_PCM_NONBLOCK as raw::c_int,
            ) {
                -16 /* determined empirically */ => return Err(BuildStreamError::DeviceNotAvailable),
                -22 => return Err(BuildStreamError::InvalidArgument),
                e => if let Err(description) = check_errors(e) {
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            }
            handle
        };
        let can_pause = unsafe {
            let hw_params = HwParams::alloc();
            set_hw_params_from_format(handle, &hw_params, conf, sample_format)
                .map_err(|description| BackendSpecificError { description })?;

            alsa::snd_pcm_hw_params_can_pause(hw_params.0) == 1
        };
        let (buffer_len, period_len) = unsafe {
            set_sw_params_from_format(handle, conf)
                .map_err(|description| BackendSpecificError { description })?
        };

        if let Err(desc) = check_errors(unsafe { alsa::snd_pcm_prepare(handle) }) {
            let description = format!("could not get handle: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let num_descriptors = {
            let num_descriptors = unsafe { alsa::snd_pcm_poll_descriptors_count(handle) };
            if num_descriptors == 0 {
                let description = "poll descriptor count for stream was 0".to_string();
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            num_descriptors as usize
        };

        let stream_inner = StreamInner {
            channel: handle,
            sample_format,
            num_descriptors,
            num_channels: conf.channels as u16,
            buffer_len,
            period_len,
            can_pause,
        };

        if let Err(desc) = check_errors(unsafe { alsa::snd_pcm_start(handle) }) {
            let description = format!("could not start stream: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        Ok(stream_inner)
    }

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.0.clone())
    }

    unsafe fn supported_configs(
        &self,
        stream_t: alsa::snd_pcm_stream_t,
    ) -> Result<VecIntoIter<SupportedStreamConfigRange>, SupportedStreamConfigsError> {
        let mut handle = ptr::null_mut();
        let device_name = match ffi::CString::new(&self.0[..]) {
            Ok(name) => name,
            Err(err) => {
                let description = format!("failed to retrieve device name: {}", err);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
        };

        match alsa::snd_pcm_open(
            &mut handle,
            device_name.as_ptr() as *const _,
            stream_t,
            alsa::SND_PCM_NONBLOCK as raw::c_int,
        ) {
            -2 |
            -16 /* determined empirically */ => return Err(SupportedStreamConfigsError::DeviceNotAvailable),
            -22 => return Err(SupportedStreamConfigsError::InvalidArgument),
            e => if let Err(description) = check_errors(e) {
                let err = BackendSpecificError { description };
                return Err(err.into())
            }
        }

        let hw_params = HwParams::alloc();
        match check_errors(alsa::snd_pcm_hw_params_any(handle, hw_params.0)) {
            Err(description) => {
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            Ok(_) => (),
        };

        // TODO: check endianess
        const FORMATS: [(SampleFormat, alsa::snd_pcm_format_t); 3] = [
            //SND_PCM_FORMAT_S8,
            //SND_PCM_FORMAT_U8,
            (
                SampleFormat::I16,
                alsa::_snd_pcm_format::SND_PCM_FORMAT_S16_LE,
            ),
            //SND_PCM_FORMAT_S16_BE,
            (
                SampleFormat::U16,
                alsa::_snd_pcm_format::SND_PCM_FORMAT_U16_LE,
            ),
            //SND_PCM_FORMAT_U16_BE,
            //SND_PCM_FORMAT_S24_LE,
            //SND_PCM_FORMAT_S24_BE,
            //SND_PCM_FORMAT_U24_LE,
            //SND_PCM_FORMAT_U24_BE,
            //SND_PCM_FORMAT_S32_LE,
            //SND_PCM_FORMAT_S32_BE,
            //SND_PCM_FORMAT_U32_LE,
            //SND_PCM_FORMAT_U32_BE,
            (
                SampleFormat::F32,
                alsa::_snd_pcm_format::SND_PCM_FORMAT_FLOAT_LE,
            ),
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
            if alsa::snd_pcm_hw_params_test_format(handle, hw_params.0, alsa_format) == 0 {
                supported_formats.push(sample_format);
            }
        }

        let mut min_rate = 0;
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_rate_min(
            hw_params.0,
            &mut min_rate,
            ptr::null_mut(),
        )) {
            let description = format!("unable to get minimum supported rate: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let mut max_rate = 0;
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_rate_max(
            hw_params.0,
            &mut max_rate,
            ptr::null_mut(),
        )) {
            let description = format!("unable to get maximum supported rate: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let sample_rates = if min_rate == max_rate
            || alsa::snd_pcm_hw_params_test_rate(handle, hw_params.0, min_rate + 1, 0) == 0
        {
            vec![(min_rate, max_rate)]
        } else {
            const RATES: [raw::c_uint; 13] = [
                5512, 8000, 11025, 16000, 22050, 32000, 44100, 48000, 64000, 88200, 96000, 176400,
                192000,
            ];

            let mut rates = Vec::new();
            for &rate in RATES.iter() {
                if alsa::snd_pcm_hw_params_test_rate(handle, hw_params.0, rate, 0) == 0 {
                    rates.push((rate, rate));
                }
            }

            if rates.len() == 0 {
                vec![(min_rate, max_rate)]
            } else {
                rates
            }
        };

        let mut min_channels = 0;
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_channels_min(
            hw_params.0,
            &mut min_channels,
        )) {
            let description = format!("unable to get minimum supported channel count: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let mut max_channels = 0;
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_channels_max(
            hw_params.0,
            &mut max_channels,
        )) {
            let description = format!("unable to get maximum supported channel count: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let max_channels = cmp::min(max_channels, 32); // TODO: limiting to 32 channels or too much stuff is returned
        let supported_channels = (min_channels..max_channels + 1)
            .filter_map(|num| {
                if alsa::snd_pcm_hw_params_test_channels(handle, hw_params.0, num) == 0 {
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

        // TODO: RAII
        alsa::snd_pcm_close(handle);
        Ok(output.into_iter())
    }

    fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        unsafe { self.supported_configs(alsa::_snd_pcm_stream::SND_PCM_STREAM_CAPTURE) }
    }

    fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        unsafe { self.supported_configs(alsa::_snd_pcm_stream::SND_PCM_STREAM_PLAYBACK) }
    }

    // ALSA does not offer default stream formats, so instead we compare all supported formats by
    // the `SupportedStreamConfigRange::cmp_default_heuristics` order and select the greatest.
    fn default_config(
        &self,
        stream_t: alsa::snd_pcm_stream_t,
    ) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut formats: Vec<_> = unsafe {
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
        self.default_config(alsa::_snd_pcm_stream::SND_PCM_STREAM_CAPTURE)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        self.default_config(alsa::_snd_pcm_stream::SND_PCM_STREAM_PLAYBACK)
    }
}

struct StreamInner {
    // The ALSA channel.
    channel: *mut alsa::snd_pcm_t,

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
unsafe impl Send for StreamInner {}

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
    descriptors: Vec<alsa::pollfd>,
    buffer: Vec<u8>,
}

fn input_stream_worker(
    rx: TriggerReceiver,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&Data) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) {
    let mut ctxt = StreamWorkerContext::default();
    loop {
        match poll_descriptors_and_prepare_buffer(&rx, stream, &mut ctxt, error_callback) {
            PollDescriptorsFlow::Continue => continue,
            PollDescriptorsFlow::Return => return,
            PollDescriptorsFlow::Ready {
                available_frames,
                stream_type,
            } => {
                assert_eq!(
                    stream_type,
                    StreamType::Input,
                    "expected input stream, but polling descriptors indicated output",
                );
                process_input(
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

fn output_stream_worker(
    rx: TriggerReceiver,
    stream: &StreamInner,
    data_callback: &mut (dyn FnMut(&mut Data) + Send + 'static),
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) {
    let mut ctxt = StreamWorkerContext::default();
    loop {
        match poll_descriptors_and_prepare_buffer(&rx, stream, &mut ctxt, error_callback) {
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
    error_callback: &mut (dyn FnMut(StreamError) + Send + 'static),
) -> PollDescriptorsFlow {
    let StreamWorkerContext {
        ref mut descriptors,
        ref mut buffer,
    } = *ctxt;

    descriptors.clear();

    // Add the self-pipe for signaling termination.
    descriptors.push(alsa::pollfd {
        fd: rx.0,
        events: alsa::POLLIN as raw::c_short,
        revents: 0,
    });

    // Add ALSA polling fds.
    descriptors.reserve(stream.num_descriptors);
    let len = descriptors.len();
    let filled = unsafe {
        alsa::snd_pcm_poll_descriptors(
            stream.channel,
            descriptors[len..].as_mut_ptr(),
            stream.num_descriptors as raw::c_uint,
        )
    };
    debug_assert_eq!(filled, stream.num_descriptors as raw::c_int);
    unsafe {
        descriptors.set_len(len + stream.num_descriptors);
    }

    let res = unsafe {
        // Don't timeout, wait forever.
        alsa::poll(
            descriptors.as_mut_ptr(),
            descriptors.len() as alsa::nfds_t,
            -1,
        )
    };
    if res < 0 {
        let description = format!("`alsa::poll()` failed: {}", io::Error::last_os_error());
        error_callback(BackendSpecificError { description }.into());
        return PollDescriptorsFlow::Continue;
    } else if res == 0 {
        let description = String::from("`alsa::poll()` spuriously returned");
        error_callback(BackendSpecificError { description }.into());
        return PollDescriptorsFlow::Continue;
    }

    if descriptors[0].revents != 0 {
        // The stream has been requested to be destroyed.
        rx.clear_pipe();
        return PollDescriptorsFlow::Return;
    }

    let stream_type = match check_for_pollout_or_pollin(stream, descriptors[1..].as_mut_ptr()) {
        Ok(Some(ty)) => ty,
        Ok(None) => {
            // Nothing to process, poll again
            return PollDescriptorsFlow::Continue;
        }
        Err(err) => {
            error_callback(err.into());
            return PollDescriptorsFlow::Continue;
        }
    };
    // Get the number of available samples for reading/writing.
    let available_samples = match get_available_samples(stream) {
        Ok(n) => n,
        Err(err) => {
            let description = format!("Failed to query the number of available samples: {}", err);
            error_callback(BackendSpecificError { description }.into());
            return PollDescriptorsFlow::Continue;
        }
    };

    // Only go on if there is at least `stream.period_len` samples.
    if available_samples < stream.period_len {
        return PollDescriptorsFlow::Continue;
    }

    // Prepare the data buffer.
    let buffer_size = stream.sample_format.sample_size() * available_samples;
    buffer.resize(buffer_size, 0u8);
    let available_frames = available_samples / stream.num_channels as usize;

    PollDescriptorsFlow::Ready {
        stream_type,
        available_frames,
    }
}

// Read input data from ALSA and deliver it to the user.
fn process_input(
    stream: &StreamInner,
    buffer: &mut [u8],
    available_frames: usize,
    data_callback: &mut (dyn FnMut(&Data) + Send + 'static),
    error_callback: &mut dyn FnMut(StreamError),
) {
    let result = unsafe {
        alsa::snd_pcm_readi(
            stream.channel,
            buffer.as_mut_ptr() as *mut _,
            available_frames as alsa::snd_pcm_uframes_t,
        )
    };
    if let Err(err) = check_errors(result as _) {
        let description = format!("`snd_pcm_readi` failed: {}", err);
        error_callback(BackendSpecificError { description }.into());
        return;
    }
    let sample_format = stream.sample_format;
    let data = buffer.as_mut_ptr() as *mut ();
    let len = buffer.len() / sample_format.sample_size();
    let data = unsafe { Data::from_parts(data, len, sample_format) };
    data_callback(&data);
}

// Request data from the user's function and write it via ALSA.
//
// Returns `true`
fn process_output(
    stream: &StreamInner,
    buffer: &mut [u8],
    available_frames: usize,
    data_callback: &mut (dyn FnMut(&mut Data) + Send + 'static),
    error_callback: &mut dyn FnMut(StreamError),
) {
    {
        // We're now sure that we're ready to write data.
        let sample_format = stream.sample_format;
        let data = buffer.as_mut_ptr() as *mut ();
        let len = buffer.len() / sample_format.sample_size();
        let mut data = unsafe { Data::from_parts(data, len, sample_format) };
        data_callback(&mut data);
    }
    loop {
        let result = unsafe {
            alsa::snd_pcm_writei(
                stream.channel,
                buffer.as_ptr() as *const _,
                available_frames as alsa::snd_pcm_uframes_t,
            )
        };
        if result == -(alsa::EPIPE as i64) {
            // buffer underrun
            // TODO: Notify the user of this.
            unsafe { alsa::snd_pcm_recover(stream.channel, result as i32, 0) };
        } else if let Err(err) = check_errors(result as _) {
            let description = format!("`snd_pcm_writei` failed: {}", err);
            error_callback(BackendSpecificError { description }.into());
            continue;
        } else if result as usize != available_frames {
            let description = format!(
                "unexpected number of frames written: expected {}, \
                 result {} (this should never happen)",
                available_frames, result,
            );
            error_callback(BackendSpecificError { description }.into());
            continue;
        } else {
            break;
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
        D: FnMut(&Data) + Send + 'static,
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
        D: FnMut(&mut Data) + Send + 'static,
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
        unsafe {
            alsa::snd_pcm_pause(self.inner.channel, 0);
        }
        // TODO: error handling
        Ok(())
    }
    fn pause(&self) -> Result<(), PauseStreamError> {
        unsafe {
            alsa::snd_pcm_pause(self.inner.channel, 1);
        }
        // TODO: error handling
        Ok(())
    }
}

// Check whether the event is `POLLOUT` or `POLLIN`.
//
// If so, return the stream type associated with the event.
//
// Otherwise, returns `Ok(None)`.
//
// Returns an `Err` if the `snd_pcm_poll_descriptors_revents` call fails.
fn check_for_pollout_or_pollin(
    stream: &StreamInner,
    stream_descriptor_ptr: *mut alsa::pollfd,
) -> Result<Option<StreamType>, BackendSpecificError> {
    let (revent, res) = unsafe {
        let mut revent = 0;
        let res = alsa::snd_pcm_poll_descriptors_revents(
            stream.channel,
            stream_descriptor_ptr,
            stream.num_descriptors as raw::c_uint,
            &mut revent,
        );
        (revent, res)
    };
    if let Err(desc) = check_errors(res) {
        let description = format!("`snd_pcm_poll_descriptors_revents` failed: {}", desc);
        let err = BackendSpecificError { description };
        return Err(err);
    }

    if revent as u32 == alsa::POLLOUT {
        Ok(Some(StreamType::Output))
    } else if revent as u32 == alsa::POLLIN {
        Ok(Some(StreamType::Input))
    } else {
        Ok(None)
    }
}

// Determine the number of samples that are available to read/write.
fn get_available_samples(stream: &StreamInner) -> Result<usize, BackendSpecificError> {
    let available = unsafe { alsa::snd_pcm_avail_update(stream.channel) };
    if available == -32 {
        // buffer underrun
        // TODO: Notify the user some how.
        Ok(stream.buffer_len)
    } else if let Err(desc) = check_errors(available as raw::c_int) {
        let description = format!("failed to get available samples: {}", desc);
        let err = BackendSpecificError { description };
        Err(err)
    } else {
        Ok((available * stream.num_channels as alsa::snd_pcm_sframes_t) as usize)
    }
}

unsafe fn set_hw_params_from_format(
    pcm_handle: *mut alsa::snd_pcm_t,
    hw_params: &HwParams,
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Result<(), String> {
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_any(pcm_handle, hw_params.0)) {
        return Err(format!("errors on pcm handle: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_access(
        pcm_handle,
        hw_params.0,
        alsa::_snd_pcm_access::SND_PCM_ACCESS_RW_INTERLEAVED,
    )) {
        return Err(format!("handle not acessible: {}", e));
    }

    let sample_format = if cfg!(target_endian = "big") {
        match sample_format {
            SampleFormat::I16 => alsa::_snd_pcm_format::SND_PCM_FORMAT_S16_BE,
            SampleFormat::U16 => alsa::_snd_pcm_format::SND_PCM_FORMAT_U16_BE,
            SampleFormat::F32 => alsa::_snd_pcm_format::SND_PCM_FORMAT_FLOAT_BE,
        }
    } else {
        match sample_format {
            SampleFormat::I16 => alsa::_snd_pcm_format::SND_PCM_FORMAT_S16_LE,
            SampleFormat::U16 => alsa::_snd_pcm_format::SND_PCM_FORMAT_U16_LE,
            SampleFormat::F32 => alsa::_snd_pcm_format::SND_PCM_FORMAT_FLOAT_LE,
        }
    };

    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_format(
        pcm_handle,
        hw_params.0,
        sample_format,
    )) {
        return Err(format!("format could not be set: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_rate(
        pcm_handle,
        hw_params.0,
        config.sample_rate.0 as raw::c_uint,
        0,
    )) {
        return Err(format!("sample rate could not be set: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_channels(
        pcm_handle,
        hw_params.0,
        config.channels as raw::c_uint,
    )) {
        return Err(format!("channel count could not be set: {}", e));
    }

    // If this isn't set manually a overlarge buffer may be used causing audio delay
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_buffer_time_near(
        pcm_handle,
        hw_params.0,
        &mut 100_000,
        &mut 0,
    )) {
        return Err(format!("buffer time could not be set: {}", e));
    }

    if let Err(e) = check_errors(alsa::snd_pcm_hw_params(pcm_handle, hw_params.0)) {
        return Err(format!("hardware params could not be set: {}", e));
    }

    Ok(())
}

unsafe fn set_sw_params_from_format(
    pcm_handle: *mut alsa::snd_pcm_t,
    config: &StreamConfig,
) -> Result<(usize, usize), String> {
    let mut sw_params = ptr::null_mut(); // TODO: RAII
    if let Err(e) = check_errors(alsa::snd_pcm_sw_params_malloc(&mut sw_params)) {
        return Err(format!("snd_pcm_sw_params_malloc failed: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_sw_params_current(pcm_handle, sw_params)) {
        return Err(format!("snd_pcm_sw_params_current failed: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_sw_params_set_start_threshold(
        pcm_handle, sw_params, 0,
    )) {
        return Err(format!(
            "snd_pcm_sw_params_set_start_threshold failed: {}",
            e
        ));
    }

    let (buffer_len, period_len) = {
        let mut buffer = 0;
        let mut period = 0;
        if let Err(e) = check_errors(alsa::snd_pcm_get_params(
            pcm_handle,
            &mut buffer,
            &mut period,
        )) {
            return Err(format!("failed to initialize buffer: {}", e));
        }
        if buffer == 0 {
            return Err(format!("initialization resulted in a null buffer"));
        }
        if let Err(e) = check_errors(alsa::snd_pcm_sw_params_set_avail_min(
            pcm_handle, sw_params, period,
        )) {
            return Err(format!("snd_pcm_sw_params_set_avail_min failed: {}", e));
        }
        let buffer = buffer as usize * config.channels as usize;
        let period = period as usize * config.channels as usize;
        (buffer, period)
    };

    if let Err(e) = check_errors(alsa::snd_pcm_sw_params(pcm_handle, sw_params)) {
        return Err(format!("snd_pcm_sw_params failed: {}", e));
    }

    alsa::snd_pcm_sw_params_free(sw_params);
    Ok((buffer_len, period_len))
}

/// Wrapper around `hw_params`.
struct HwParams(*mut alsa::snd_pcm_hw_params_t);

impl HwParams {
    pub fn alloc() -> HwParams {
        unsafe {
            let mut hw_params = ptr::null_mut();
            check_errors(alsa::snd_pcm_hw_params_malloc(&mut hw_params))
                .expect("unable to get hardware parameters");
            HwParams(hw_params)
        }
    }
}

impl Drop for HwParams {
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_hw_params_free(self.0);
        }
    }
}

impl Drop for StreamInner {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_close(self.channel);
        }
    }
}

#[inline]
fn check_errors(err: raw::c_int) -> Result<(), String> {
    if err < 0 {
        unsafe {
            let s = ffi::CStr::from_ptr(alsa::snd_strerror(err))
                .to_bytes()
                .to_vec();
            let s = String::from_utf8(s).expect("Streaming error occured");
            return Err(s);
        }
    }

    Ok(())
}
