extern crate alsa_sys as alsa;
extern crate libc;

pub use self::enumerate::{Devices, default_input_device, default_output_device};

use ChannelCount;
use BackendSpecificError;
use BuildStreamError;
use DefaultFormatError;
use DeviceNameError;
use Format;
use PauseStreamError;
use PlayStreamError;
use SupportedFormatsError;
use SampleFormat;
use SampleRate;
use StreamData;
use StreamError;
use StreamEvent;
use SupportedFormat;
use UnknownTypeInputBuffer;
use UnknownTypeOutputBuffer;

use std::{cmp, ffi, mem, ptr};
use std::sync::Mutex;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::vec::IntoIter as VecIntoIter;

pub type SupportedInputFormats = VecIntoIter<SupportedFormat>;
pub type SupportedOutputFormats = VecIntoIter<SupportedFormat>;

mod enumerate;


struct Trigger {
    // [read fd, write fd]
    fds: [libc::c_int; 2],
}

impl Trigger {
    fn new() -> Self {
        let mut fds = [0, 0];
        match unsafe { libc::pipe(fds.as_mut_ptr()) } {
            0 => Trigger { fds: fds },
            _ => panic!("Could not create pipe"),
        }
    }
    fn read_fd(&self) -> libc::c_int {
        self.fds[0]
    }
    fn write_fd(&self) -> libc::c_int {
        self.fds[1]
    }
    fn wakeup(&self) {
        let buf = 1u64;
        let ret = unsafe { libc::write(self.write_fd(), &buf as *const u64 as *const _, 8) };
        assert!(ret == 8);
    }
    fn clear_pipe(&self) {
        let mut out = 0u64;
        let ret = unsafe { libc::read(self.read_fd(), &mut out as *mut u64 as *mut _, 8) };
        assert_eq!(ret, 8);
    }
}

impl Drop for Trigger {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fds[0]);
            libc::close(self.fds[1]);
        }
    }
}


#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device(String);

impl Device {
    #[inline]
    pub fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.0.clone())
    }

    unsafe fn supported_formats(
        &self,
        stream_t: alsa::snd_pcm_stream_t,
    ) -> Result<VecIntoIter<SupportedFormat>, SupportedFormatsError>
    {
        let mut handle = mem::uninitialized();
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
            alsa::SND_PCM_NONBLOCK,
        ) {
            -2 |
            -16 /* determined empirically */ => return Err(SupportedFormatsError::DeviceNotAvailable),
            -22 => return Err(SupportedFormatsError::InvalidArgument),
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
        const FORMATS: [(SampleFormat, alsa::snd_pcm_format_t); 3] =
            [
                //SND_PCM_FORMAT_S8,
                //SND_PCM_FORMAT_U8,
                (SampleFormat::I16, alsa::SND_PCM_FORMAT_S16_LE),
                //SND_PCM_FORMAT_S16_BE,
                (SampleFormat::U16, alsa::SND_PCM_FORMAT_U16_LE),
                //SND_PCM_FORMAT_U16_BE,
            /*SND_PCM_FORMAT_S24_LE,
            SND_PCM_FORMAT_S24_BE,
            SND_PCM_FORMAT_U24_LE,
            SND_PCM_FORMAT_U24_BE,
            SND_PCM_FORMAT_S32_LE,
            SND_PCM_FORMAT_S32_BE,
            SND_PCM_FORMAT_U32_LE,
            SND_PCM_FORMAT_U32_BE,*/
                (SampleFormat::F32, alsa::SND_PCM_FORMAT_FLOAT_LE) /*SND_PCM_FORMAT_FLOAT_BE,
            SND_PCM_FORMAT_FLOAT64_LE,
            SND_PCM_FORMAT_FLOAT64_BE,
            SND_PCM_FORMAT_IEC958_SUBFRAME_LE,
            SND_PCM_FORMAT_IEC958_SUBFRAME_BE,
            SND_PCM_FORMAT_MU_LAW,
            SND_PCM_FORMAT_A_LAW,
            SND_PCM_FORMAT_IMA_ADPCM,
            SND_PCM_FORMAT_MPEG,
            SND_PCM_FORMAT_GSM,
            SND_PCM_FORMAT_SPECIAL,
            SND_PCM_FORMAT_S24_3LE,
            SND_PCM_FORMAT_S24_3BE,
            SND_PCM_FORMAT_U24_3LE,
            SND_PCM_FORMAT_U24_3BE,
            SND_PCM_FORMAT_S20_3LE,
            SND_PCM_FORMAT_S20_3BE,
            SND_PCM_FORMAT_U20_3LE,
            SND_PCM_FORMAT_U20_3BE,
            SND_PCM_FORMAT_S18_3LE,
            SND_PCM_FORMAT_S18_3BE,
            SND_PCM_FORMAT_U18_3LE,
            SND_PCM_FORMAT_U18_3BE,*/,
            ];

        let mut supported_formats = Vec::new();
        for &(sample_format, alsa_format) in FORMATS.iter() {
            if alsa::snd_pcm_hw_params_test_format(handle,
                                                   hw_params.0,
                                                   alsa_format) == 0
            {
                supported_formats.push(sample_format);
            }
        }

        let mut min_rate = mem::uninitialized();
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_rate_min(
            hw_params.0,
            &mut min_rate,
            ptr::null_mut(),
        )) {
            let description = format!("unable to get minimum supported rate: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let mut max_rate = mem::uninitialized();
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_rate_max(
            hw_params.0,
            &mut max_rate,
            ptr::null_mut(),
        )) {
            let description = format!("unable to get maximum supported rate: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let sample_rates = if min_rate == max_rate {
            vec![(min_rate, max_rate)]
        } else if alsa::snd_pcm_hw_params_test_rate(handle,
                                                    hw_params.0,
                                                    min_rate + 1,
                                                    0) == 0
        {
            vec![(min_rate, max_rate)]
        } else {
            const RATES: [libc::c_uint; 13] = [
                5512,
                8000,
                11025,
                16000,
                22050,
                32000,
                44100,
                48000,
                64000,
                88200,
                96000,
                176400,
                192000,
            ];

            let mut rates = Vec::new();
            for &rate in RATES.iter() {
                if alsa::snd_pcm_hw_params_test_rate(handle,
                                                     hw_params.0,
                                                     rate,
                                                     0) == 0
                {
                    rates.push((rate, rate));
                }
            }

            if rates.len() == 0 {
                vec![(min_rate, max_rate)]
            } else {
                rates
            }
        };

        let mut min_channels = mem::uninitialized();
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_channels_min(hw_params.0, &mut min_channels)) {
            let description = format!("unable to get minimum supported channel count: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let mut max_channels = mem::uninitialized();
        if let Err(desc) = check_errors(alsa::snd_pcm_hw_params_get_channels_max(hw_params.0, &mut max_channels)) {
            let description = format!("unable to get maximum supported channel count: {}", desc);
            let err = BackendSpecificError { description };
            return Err(err.into());
        }

        let max_channels = cmp::min(max_channels, 32); // TODO: limiting to 32 channels or too much stuff is returned
        let supported_channels = (min_channels .. max_channels + 1)
            .filter_map(|num| if alsa::snd_pcm_hw_params_test_channels(
                handle,
                hw_params.0,
                num,
            ) == 0
            {
                Some(num as ChannelCount)
            } else {
                None
            })
            .collect::<Vec<_>>();

        let mut output = Vec::with_capacity(supported_formats.len() * supported_channels.len() *
                                                sample_rates.len());
        for &data_type in supported_formats.iter() {
            for channels in supported_channels.iter() {
                for &(min_rate, max_rate) in sample_rates.iter() {
                    output.push(SupportedFormat {
                                    channels: channels.clone(),
                                    min_sample_rate: SampleRate(min_rate as u32),
                                    max_sample_rate: SampleRate(max_rate as u32),
                                    data_type: data_type,
                                });
                }
            }
        }

        // TODO: RAII
        alsa::snd_pcm_close(handle);
        Ok(output.into_iter())
    }

    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, SupportedFormatsError> {
        unsafe {
            self.supported_formats(alsa::SND_PCM_STREAM_CAPTURE)
        }
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        unsafe {
            self.supported_formats(alsa::SND_PCM_STREAM_PLAYBACK)
        }
    }

    // ALSA does not offer default stream formats, so instead we compare all supported formats by
    // the `SupportedFormat::cmp_default_heuristics` order and select the greatest.
    fn default_format(
        &self,
        stream_t: alsa::snd_pcm_stream_t,
    ) -> Result<Format, DefaultFormatError>
    {
        let mut formats: Vec<_> = unsafe {
            match self.supported_formats(stream_t) {
                Err(SupportedFormatsError::DeviceNotAvailable) => {
                    return Err(DefaultFormatError::DeviceNotAvailable);
                },
                Err(SupportedFormatsError::InvalidArgument) => {
                    // this happens sometimes when querying for input and output capabilities but
                    // the device supports only one
                    return Err(DefaultFormatError::StreamTypeNotSupported);
                }
                Err(SupportedFormatsError::BackendSpecific { err }) => {
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
            },
            None => Err(DefaultFormatError::StreamTypeNotSupported)
        }
    }

    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        self.default_format(alsa::SND_PCM_STREAM_CAPTURE)
    }

    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        self.default_format(alsa::SND_PCM_STREAM_PLAYBACK)
    }
}

pub struct EventLoop {
    // Each newly-created stream gets a new ID from this counter. The counter is then incremented.
    next_stream_id: AtomicUsize, // TODO: use AtomicU64 when stable?

    // A trigger that uses a `pipe()` as backend. Signalled whenever a new command is ready, so
    // that `poll()` can wake up and pick the changes.
    pending_command_trigger: Trigger,

    // This field is locked by the `run()` method.
    // The mutex also ensures that only one thread at a time has `run()` running.
    run_context: Mutex<RunContext>,

    // Commands processed by the `run()` method that is currently running.
    commands: Sender<Command>,
}

unsafe impl Send for EventLoop {
}

unsafe impl Sync for EventLoop {
}

enum Command {
    NewStream(StreamInner),
    PlayStream(StreamId),
    PauseStream(StreamId),
    DestroyStream(StreamId),
}

struct RunContext {
    // Descriptors to wait for. Always contains `pending_command_trigger.read_fd()` as first element.
    descriptors: Vec<libc::pollfd>,
    // List of streams that are written in `descriptors`.
    streams: Vec<StreamInner>,

    commands: Receiver<Command>,
}

struct StreamInner {
    // The id of the stream.
    id: StreamId,

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

    // Whether or not the sample stream is currently paused.
    is_paused: bool,

    // A file descriptor opened with `eventfd`.
    // It is used to wait for resume signal.
    resume_trigger: Trigger,

    // Lazily allocated buffer that is reused inside the loop.
    // Zero-allocate a new buffer (the fastest way to have zeroed memory) at the first time this is
    // used.
    buffer: Vec<u8>,
}

#[derive(Copy, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

enum StreamType { Input, Output }


impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        let pending_command_trigger = Trigger::new();

        let mut initial_descriptors = vec![];
        reset_descriptors_with_pending_command_trigger(
            &mut initial_descriptors,
            &pending_command_trigger,
        );

        let (tx, rx) = channel();

        let run_context = Mutex::new(RunContext {
                                         descriptors: initial_descriptors,
                                         streams: Vec::new(),
                                         commands: rx,
                                     });

        EventLoop {
            next_stream_id: AtomicUsize::new(0),
            pending_command_trigger: pending_command_trigger,
            run_context,
            commands: tx,
        }
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamEvent)
    {
        self.run_inner(&mut callback)
    }

    fn run_inner(&self, callback: &mut dyn FnMut(StreamId, StreamEvent)) -> ! {
        unsafe {
            let mut run_context = self.run_context.lock().unwrap();
            let run_context = &mut *run_context;

            'stream_loop: loop {
                process_commands(run_context);

                reset_descriptors_with_pending_command_trigger(
                    &mut run_context.descriptors,
                    &self.pending_command_trigger,
                );
                append_stream_poll_descriptors(run_context);

                // At this point, this should include the command `pending_commands_trigger` along
                // with the poll descriptors for each stream.
                match poll_all_descriptors(&mut run_context.descriptors) {
                    Ok(true) => (),
                    Ok(false) => continue,
                    Err(err) => {
                        for stream in run_context.streams.iter() {
                            let event = StreamEvent::Close(err.clone().into());
                            callback(stream.id, event);
                        }
                        run_context.streams.clear();
                        break 'stream_loop;
                    }
                }

                // If the `pending_command_trigger` was signaled, we need to process the comands.
                if run_context.descriptors[0].revents != 0 {
                    run_context.descriptors[0].revents = 0;
                    self.pending_command_trigger.clear_pipe();
                }

                // The set of streams that error within the following loop and should be removed.
                let mut streams_to_remove: Vec<(StreamId, StreamError)> = vec![];

                // Iterate over each individual stream/descriptor.
                let mut i_stream = 0;
                let mut i_descriptor = 1;
                while (i_descriptor as usize) < run_context.descriptors.len() {
                    let stream = &mut run_context.streams[i_stream];
                    let stream_descriptor_ptr = run_context.descriptors.as_mut_ptr().offset(i_descriptor);

                    // Only go on if this event was a pollout or pollin event.
                    let stream_type = match check_for_pollout_or_pollin(stream, stream_descriptor_ptr) {
                        Ok(Some(ty)) => ty,
                        Ok(None) => {
                            i_descriptor += stream.num_descriptors as isize;
                            i_stream += 1;
                            continue;
                        },
                        Err(err) => {
                            streams_to_remove.push((stream.id, err.into()));
                            i_descriptor += stream.num_descriptors as isize;
                            i_stream += 1;
                            continue;
                        }
                    };

                    // Get the number of available samples for reading/writing.
                    let available_samples = match get_available_samples(stream) {
                        Ok(n) => n,
                        Err(err) => {
                            streams_to_remove.push((stream.id, err.into()));
                            i_descriptor += stream.num_descriptors as isize;
                            i_stream += 1;
                            continue;
                        }
                    };

                    // Only go on if there is at least `stream.period_len` samples.
                    if available_samples < stream.period_len {
                        i_descriptor += stream.num_descriptors as isize;
                        i_stream += 1;
                        continue;
                    }

                    // Prepare the data buffer.
                    let buffer_size = stream.sample_format.sample_size() * available_samples;
                    stream.buffer.resize(buffer_size, 0u8);
                    let available_frames = available_samples / stream.num_channels as usize;

                    match stream_type {
                        StreamType::Input => {
                            let result = alsa::snd_pcm_readi(
                                stream.channel,
                                stream.buffer.as_mut_ptr() as *mut _,
                                available_frames as alsa::snd_pcm_uframes_t,
                            );
                            if let Err(err) = check_errors(result as _) {
                                let description = format!("`snd_pcm_readi` failed: {}", err);
                                let err = BackendSpecificError { description };
                                streams_to_remove.push((stream.id, err.into()));
                                continue;
                            }

                            let input_buffer = match stream.sample_format {
                                SampleFormat::I16 => UnknownTypeInputBuffer::I16(::InputBuffer {
                                    buffer: cast_input_buffer(&mut stream.buffer),
                                }),
                                SampleFormat::U16 => UnknownTypeInputBuffer::U16(::InputBuffer {
                                    buffer: cast_input_buffer(&mut stream.buffer),
                                }),
                                SampleFormat::F32 => UnknownTypeInputBuffer::F32(::InputBuffer {
                                    buffer: cast_input_buffer(&mut stream.buffer),
                                }),
                            };
                            let stream_data = StreamData::Input {
                                buffer: input_buffer,
                            };
                            let event = StreamEvent::Data(stream_data);
                            callback(stream.id, event);
                        },
                        StreamType::Output => {
                            {
                                // We're now sure that we're ready to write data.
                                let output_buffer = match stream.sample_format {
                                    SampleFormat::I16 => UnknownTypeOutputBuffer::I16(::OutputBuffer {
                                        buffer: cast_output_buffer(&mut stream.buffer),
                                    }),
                                    SampleFormat::U16 => UnknownTypeOutputBuffer::U16(::OutputBuffer {
                                        buffer: cast_output_buffer(&mut stream.buffer),
                                    }),
                                    SampleFormat::F32 => UnknownTypeOutputBuffer::F32(::OutputBuffer {
                                        buffer: cast_output_buffer(&mut stream.buffer),
                                    }),
                                };

                                let stream_data = StreamData::Output {
                                    buffer: output_buffer,
                                };
                                let event = StreamEvent::Data(stream_data);
                                callback(stream.id, event);
                            }
                            loop {
                                let result = alsa::snd_pcm_writei(
                                    stream.channel,
                                    stream.buffer.as_ptr() as *const _,
                                    available_frames as alsa::snd_pcm_uframes_t,
                                );

                                if result == -32 {
                                    // buffer underrun
                                    // TODO: Notify the user of this.
                                    alsa::snd_pcm_prepare(stream.channel);
                                } else if let Err(err) = check_errors(result as _) {
                                    let description = format!("`snd_pcm_writei` failed: {}", err);
                                    let err = BackendSpecificError { description };
                                    streams_to_remove.push((stream.id, err.into()));
                                    continue;
                                } else if result as usize != available_frames {
                                    let description = format!(
                                        "unexpected number of frames written: expected {}, \
                                        result {} (this should never happen)",
                                        available_frames,
                                        result,
                                    );
                                    let err = BackendSpecificError { description };
                                    streams_to_remove.push((stream.id, err.into()));
                                    continue;
                                } else {
                                    break;
                                }
                            }
                        },
                    }
                }

                // Remove any streams that have errored and notify the user.
                for (stream_id, err) in streams_to_remove {
                    run_context.streams.retain(|s| s.id != stream_id);
                    let event = StreamEvent::Close(err.into());
                    callback(stream_id, event);
                }
            }
        }

        panic!("`cpal::EventLoop::run` API currently disallows returning");
    }

    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        unsafe {
            let name = ffi::CString::new(device.0.clone()).expect("unable to clone device");

            let mut capture_handle = mem::uninitialized();
            match alsa::snd_pcm_open(
                &mut capture_handle,
                name.as_ptr(),
                alsa::SND_PCM_STREAM_CAPTURE,
                alsa::SND_PCM_NONBLOCK,
            ) {
                -16 /* determined empirically */ => return Err(BuildStreamError::DeviceNotAvailable),
                -22 => return Err(BuildStreamError::InvalidArgument),
                e => if let Err(description) = check_errors(e) {
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            }
            let hw_params = HwParams::alloc();

            set_hw_params_from_format(capture_handle, &hw_params, format)
                .map_err(|description| BackendSpecificError { description })?;

            let can_pause = alsa::snd_pcm_hw_params_can_pause(hw_params.0) == 1;

            let (buffer_len, period_len) = set_sw_params_from_format(capture_handle, format)
                .map_err(|description| BackendSpecificError { description })?;

            if let Err(desc) = check_errors(alsa::snd_pcm_prepare(capture_handle)) {
                let description = format!("could not get capture handle: {}", desc);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }

            let num_descriptors = {
                let num_descriptors = alsa::snd_pcm_poll_descriptors_count(capture_handle);
                if num_descriptors == 0 {
                    let description = "poll descriptor count for capture stream was 0".to_string();
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
                num_descriptors as usize
            };

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            if new_stream_id.0 == usize::max_value() {
                return Err(BuildStreamError::StreamIdOverflow);
            }

            let stream_inner = StreamInner {
                id: new_stream_id.clone(),
                channel: capture_handle,
                sample_format: format.data_type,
                num_descriptors: num_descriptors,
                num_channels: format.channels as u16,
                buffer_len: buffer_len,
                period_len: period_len,
                can_pause: can_pause,
                is_paused: false,
                resume_trigger: Trigger::new(),
                buffer: vec![],
            };

            if let Err(desc) = check_errors(alsa::snd_pcm_start(capture_handle)) {
                let description = format!("could not start capture stream: {}", desc);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }

            self.push_command(Command::NewStream(stream_inner));
            Ok(new_stream_id)
        }
    }

    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        unsafe {
            let name = ffi::CString::new(device.0.clone()).expect("unable to clone device");

            let mut playback_handle = mem::uninitialized();
            match alsa::snd_pcm_open(
                &mut playback_handle,
                name.as_ptr(),
                alsa::SND_PCM_STREAM_PLAYBACK,
                alsa::SND_PCM_NONBLOCK,
            ) {
                -16 /* determined empirically */ => return Err(BuildStreamError::DeviceNotAvailable),
                -22 => return Err(BuildStreamError::InvalidArgument),
                e => if let Err(description) = check_errors(e) {
                    let err = BackendSpecificError { description };
                    return Err(err.into())
                }
            }
            let hw_params = HwParams::alloc();

            set_hw_params_from_format(playback_handle, &hw_params, format)
                .map_err(|description| BackendSpecificError { description })?;

            let can_pause = alsa::snd_pcm_hw_params_can_pause(hw_params.0) == 1;

            let (buffer_len, period_len) = set_sw_params_from_format(playback_handle, format)
                .map_err(|description| BackendSpecificError { description })?;

            if let Err(desc) = check_errors(alsa::snd_pcm_prepare(playback_handle)) {
                let description = format!("could not get playback handle: {}", desc);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }

            let num_descriptors = {
                let num_descriptors = alsa::snd_pcm_poll_descriptors_count(playback_handle);
                if num_descriptors == 0 {
                    let description = "poll descriptor count for playback stream was 0".to_string();
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
                num_descriptors as usize
            };

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            if new_stream_id.0 == usize::max_value() {
                return Err(BuildStreamError::StreamIdOverflow);
            }

            let stream_inner = StreamInner {
                id: new_stream_id.clone(),
                channel: playback_handle,
                sample_format: format.data_type,
                num_descriptors: num_descriptors,
                num_channels: format.channels as u16,
                buffer_len: buffer_len,
                period_len: period_len,
                can_pause: can_pause,
                is_paused: false,
                resume_trigger: Trigger::new(),
                buffer: vec![],
            };

            self.push_command(Command::NewStream(stream_inner));
            Ok(new_stream_id)
        }
    }

    #[inline]
    fn push_command(&self, command: Command) {
        // Safe to unwrap: sender outlives receiver.
        self.commands.send(command).unwrap();
        self.pending_command_trigger.wakeup();
    }

    #[inline]
    pub fn destroy_stream(&self, stream_id: StreamId) {
        self.push_command(Command::DestroyStream(stream_id));
    }

    #[inline]
    pub fn play_stream(&self, stream_id: StreamId) -> Result<(), PlayStreamError> {
        self.push_command(Command::PlayStream(stream_id));
        Ok(())
    }

    #[inline]
    pub fn pause_stream(&self, stream_id: StreamId) -> Result<(), PauseStreamError> {
        self.push_command(Command::PauseStream(stream_id));
        Ok(())
    }
}

// Process any pending `Command`s within the `RunContext`'s queue.
fn process_commands(run_context: &mut RunContext) {
    for command in run_context.commands.try_iter() {
        match command {
            Command::DestroyStream(stream_id) => {
                run_context.streams.retain(|s| s.id != stream_id);
            },
            Command::PlayStream(stream_id) => {
                if let Some(stream) = run_context.streams.iter_mut()
                    .find(|stream| stream.can_pause && stream.id == stream_id)
                {
                    unsafe {
                        alsa::snd_pcm_pause(stream.channel, 0);
                    }
                    stream.is_paused = false;
                }
            },
            Command::PauseStream(stream_id) => {
                if let Some(stream) = run_context.streams.iter_mut()
                    .find(|stream| stream.can_pause && stream.id == stream_id)
                {
                    unsafe {
                        alsa::snd_pcm_pause(stream.channel, 1);
                    }
                    stream.is_paused = true;
                }
            },
            Command::NewStream(stream_inner) => {
                run_context.streams.push(stream_inner);
            },
        }
    }
}

// Resets the descriptors so that only `pending_command_trigger.read_fd()` is contained.
fn reset_descriptors_with_pending_command_trigger(
    descriptors: &mut Vec<libc::pollfd>,
    pending_command_trigger: &Trigger,
) {
    descriptors.clear();
    descriptors.push(libc::pollfd {
        fd: pending_command_trigger.read_fd(),
        events: libc::POLLIN,
        revents: 0,
    });
}

// Appends the `poll` descriptors for each stream onto the `RunContext`'s descriptor slice, ready
// for a call to `libc::poll`.
fn append_stream_poll_descriptors(run_context: &mut RunContext) {
    for stream in run_context.streams.iter() {
        run_context.descriptors.reserve(stream.num_descriptors);
        let len = run_context.descriptors.len();
        let filled = unsafe {
            alsa::snd_pcm_poll_descriptors(
                stream.channel,
                run_context.descriptors.as_mut_ptr().offset(len as isize),
                stream.num_descriptors as libc::c_uint,
            )
        };
        debug_assert_eq!(filled, stream.num_descriptors as libc::c_int);
        unsafe {
            run_context.descriptors.set_len(len + stream.num_descriptors);
        }
    }
}

// Poll all descriptors within the given set.
//
// Returns `Ok(true)` if some event has occurred or `Ok(false)` if no events have
// occurred.
//
// Returns an `Err` if `libc::poll` returns a negative value for some reason.
fn poll_all_descriptors(descriptors: &mut [libc::pollfd]) -> Result<bool, BackendSpecificError> {
    let res = unsafe {
        // Don't timeout, wait forever.
        libc::poll(descriptors.as_mut_ptr(), descriptors.len() as libc::nfds_t, -1)
    };
    if res < 0 {
        let description = format!("`libc::poll()` failed: {}", res);
        Err(BackendSpecificError { description })
    } else if res == 0 {
        Ok(false)
    } else {
        Ok(true)
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
    stream_descriptor_ptr: *mut libc::pollfd,
) -> Result<Option<StreamType>, BackendSpecificError> {
    let (revent, res) = unsafe {
        let mut revent = mem::uninitialized();
        let res = alsa::snd_pcm_poll_descriptors_revents(
            stream.channel,
            stream_descriptor_ptr,
            stream.num_descriptors as libc::c_uint,
            &mut revent,
        );
        (revent, res)
    };
    if let Err(desc) = check_errors(res) {
        let description =
            format!("`snd_pcm_poll_descriptors_revents` failed: {}",desc);
        let err = BackendSpecificError { description };
        return Err(err);
    }

    if revent as i16 == libc::POLLOUT {
        Ok(Some(StreamType::Output))
    } else if revent as i16 == libc::POLLIN {
        Ok(Some(StreamType::Input))
    } else {
        Ok(None)
    }
}

// Determine the number of samples that are available to read/write.
fn get_available_samples(stream: &StreamInner) -> Result<usize, BackendSpecificError> {
    // TODO: what about snd_pcm_avail_update?
    let available = unsafe {
        alsa::snd_pcm_avail(stream.channel)
    };
    if available == -32 {
        // buffer underrun
        // TODO: Notify the user some how.
        Ok(stream.buffer_len)
    } else if let Err(desc) = check_errors(available as libc::c_int) {
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
    format: &Format,
) -> Result<(), String> {
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_any(pcm_handle, hw_params.0)) {
        return Err(format!("errors on pcm handle: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_access(pcm_handle,
                                                    hw_params.0,
                                                    alsa::SND_PCM_ACCESS_RW_INTERLEAVED)) {
        return Err(format!("handle not acessible: {}", e));
    }

    let data_type = if cfg!(target_endian = "big") {
        match format.data_type {
            SampleFormat::I16 => alsa::SND_PCM_FORMAT_S16_BE,
            SampleFormat::U16 => alsa::SND_PCM_FORMAT_U16_BE,
            SampleFormat::F32 => alsa::SND_PCM_FORMAT_FLOAT_BE,
        }
    } else {
        match format.data_type {
            SampleFormat::I16 => alsa::SND_PCM_FORMAT_S16_LE,
            SampleFormat::U16 => alsa::SND_PCM_FORMAT_U16_LE,
            SampleFormat::F32 => alsa::SND_PCM_FORMAT_FLOAT_LE,
        }
    };

    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_format(pcm_handle,
                                                    hw_params.0,
                                                    data_type)) {
        return Err(format!("format could not be set: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_rate(pcm_handle,
                                                  hw_params.0,
                                                  format.sample_rate.0 as libc::c_uint,
                                                  0)) {
        return Err(format!("sample rate could not be set: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_channels(pcm_handle,
                                                      hw_params.0,
                                                      format.channels as
                                                                      libc::c_uint)) {
        return Err(format!("channel count could not be set: {}", e));
    }

    // TODO: Review this. 200ms seems arbitrary...
    let mut max_buffer_size = format.sample_rate.0 as alsa::snd_pcm_uframes_t /
        format.channels as alsa::snd_pcm_uframes_t /
        5; // 200ms of buffer
    if let Err(e) = check_errors(alsa::snd_pcm_hw_params_set_buffer_size_max(pcm_handle,
                                                             hw_params.0,
                                                             &mut max_buffer_size))
    {
        return Err(format!("max buffer size could not be set: {}", e));
    }

    if let Err(e) = check_errors(alsa::snd_pcm_hw_params(pcm_handle, hw_params.0)) {
        return Err(format!("hardware params could not be set: {}", e));
    }

    Ok(())
}

unsafe fn set_sw_params_from_format(
    pcm_handle: *mut alsa::snd_pcm_t,
    format: &Format,
) -> Result<(usize, usize), String>
{
    let mut sw_params = mem::uninitialized(); // TODO: RAII
    if let Err(e) = check_errors(alsa::snd_pcm_sw_params_malloc(&mut sw_params)) {
        return Err(format!("snd_pcm_sw_params_malloc failed: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_sw_params_current(pcm_handle, sw_params)) {
        return Err(format!("snd_pcm_sw_params_current failed: {}", e));
    }
    if let Err(e) = check_errors(alsa::snd_pcm_sw_params_set_start_threshold(pcm_handle, sw_params, 0)) {
        return Err(format!("snd_pcm_sw_params_set_start_threshold failed: {}", e));
    }

    let (buffer_len, period_len) = {
        let mut buffer = mem::uninitialized();
        let mut period = mem::uninitialized();
        if let Err(e) = check_errors(alsa::snd_pcm_get_params(pcm_handle, &mut buffer, &mut period)) {
            return Err(format!("failed to initialize buffer: {}", e));
        }
        if buffer == 0 {
            return Err(format!("initialization resulted in a null buffer"));
        }
        if let Err(e) = check_errors(alsa::snd_pcm_sw_params_set_avail_min(pcm_handle, sw_params, period)) {
            return Err(format!("snd_pcm_sw_params_set_avail_min failed: {}", e));
        }
        let buffer = buffer as usize * format.channels as usize;
        let period = period as usize * format.channels as usize;
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
            let mut hw_params = mem::uninitialized();
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
fn check_errors(err: libc::c_int) -> Result<(), String> {
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

/// Cast a byte slice into a (immutable) slice of desired type.
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_input_buffer<T>(v: &[u8]) -> &[T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts(v.as_ptr() as *const T, v.len() / std::mem::size_of::<T>())
}

/// Cast a byte slice into a mutable slice of desired type.
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_output_buffer<T>(v: &mut [u8]) -> &mut [T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut T, v.len() / std::mem::size_of::<T>())
}
