extern crate alsa_sys as alsa;
extern crate libc;

pub use self::enumerate::{Devices, default_input_device, default_output_device};

use ChannelCount;
use CreationError;
use DefaultFormatError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SampleRate;
use StreamData;
use SupportedFormat;
use UnknownTypeInputBuffer;
use UnknownTypeOutputBuffer;

use std::{cmp, ffi, iter, mem, ptr};
use std::sync::Mutex;
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
    pub fn name(&self) -> String {
        self.0.clone()
    }

    unsafe fn supported_formats(
        &self,
        stream_t: alsa::snd_pcm_stream_t,
    ) -> Result<VecIntoIter<SupportedFormat>, FormatsEnumerationError>
    {
        let mut handle = mem::uninitialized();
        let device_name = ffi::CString::new(&self.0[..]).expect("Unable to get device name");

        match alsa::snd_pcm_open(
            &mut handle,
            device_name.as_ptr() as *const _,
            stream_t,
            alsa::SND_PCM_NONBLOCK,
        ) {
            -2 |
            -16 |
            -22 /* determined empirically */ => return Err(FormatsEnumerationError::DeviceNotAvailable),
            e => check_errors(e).expect("device not available")
        }

        let hw_params = HwParams::alloc();
        match check_errors(alsa::snd_pcm_hw_params_any(handle, hw_params.0)) {
            Err(_) => return Ok(Vec::new().into_iter()),
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
        check_errors(alsa::snd_pcm_hw_params_get_rate_min(hw_params.0,
                                                          &mut min_rate,
                                                          ptr::null_mut()))
            .expect("unable to get minimum supported rete");
        let mut max_rate = mem::uninitialized();
        check_errors(alsa::snd_pcm_hw_params_get_rate_max(hw_params.0,
                                                          &mut max_rate,
                                                          ptr::null_mut()))
            .expect("unable to get maximum supported rate");

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
        check_errors(alsa::snd_pcm_hw_params_get_channels_min(hw_params.0, &mut min_channels))
            .expect("unable to get minimum supported channel count");
        let mut max_channels = mem::uninitialized();
        check_errors(alsa::snd_pcm_hw_params_get_channels_max(hw_params.0, &mut max_channels))
            .expect("unable to get maximum supported channel count");
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

    pub fn supported_input_formats(&self) -> Result<SupportedInputFormats, FormatsEnumerationError> {
        unsafe {
            self.supported_formats(alsa::SND_PCM_STREAM_CAPTURE)
        }
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, FormatsEnumerationError> {
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
                Err(FormatsEnumerationError::DeviceNotAvailable) => {
                    return Err(DefaultFormatError::DeviceNotAvailable);
                },
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
    pending_trigger: Trigger,

    // This field is locked by the `run()` method.
    // The mutex also ensures that only one thread at a time has `run()` running.
    run_context: Mutex<RunContext>,

    // Commands processed by the `run()` method that is currently running.
    // TODO: use a lock-free container
    commands: Mutex<Vec<Command>>,
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
    // Descriptors to wait for. Always contains `pending_trigger.read_fd()` as first element.
    descriptors: Vec<libc::pollfd>,
    // List of streams that are written in `descriptors`.
    streams: Vec<StreamInner>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        let pending_trigger = Trigger::new();

        let initial_descriptors = vec![
            libc::pollfd {
                fd: pending_trigger.read_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let run_context = Mutex::new(RunContext {
                                         descriptors: initial_descriptors,
                                         streams: Vec::new(),
                                     });

        EventLoop {
            next_stream_id: AtomicUsize::new(0),
            pending_trigger: pending_trigger,
            run_context,
            commands: Mutex::new(Vec::new()),
        }
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData)
    {
        self.run_inner(&mut callback)
    }

    fn run_inner(&self, callback: &mut FnMut(StreamId, StreamData)) -> ! {
        unsafe {
            let mut run_context = self.run_context.lock().unwrap();
            let run_context = &mut *run_context;

            loop {
                {
                    let mut commands_lock = self.commands.lock().unwrap();
                    if !commands_lock.is_empty() {
                        for command in commands_lock.drain(..) {
                            match command {
                                Command::DestroyStream(stream_id) => {
                                    run_context.streams.retain(|s| s.id != stream_id);
                                },
                                Command::PlayStream(stream_id) => {
                                    if let Some(stream) = run_context.streams.iter_mut()
                                        .find(|stream| stream.can_pause && stream.id == stream_id)
                                    {
                                        alsa::snd_pcm_pause(stream.channel, 0);
                                        stream.is_paused = false;
                                    }
                                },
                                Command::PauseStream(stream_id) => {
                                    if let Some(stream) = run_context.streams.iter_mut()
                                        .find(|stream| stream.can_pause && stream.id == stream_id)
                                    {
                                        alsa::snd_pcm_pause(stream.channel, 1);
                                        stream.is_paused = true;
                                    }
                                },
                                Command::NewStream(stream_inner) => {
                                    run_context.streams.push(stream_inner);
                                },
                            }
                        }

                        run_context.descriptors = vec![
                            libc::pollfd {
                                fd: self.pending_trigger.read_fd(),
                                events: libc::POLLIN,
                                revents: 0,
                            },
                        ];
                        for stream in run_context.streams.iter() {
                            run_context.descriptors.reserve(stream.num_descriptors);
                            let len = run_context.descriptors.len();
                            let filled = alsa::snd_pcm_poll_descriptors(stream.channel,
                                                                        run_context
                                                                            .descriptors
                                                                            .as_mut_ptr()
                                                                            .offset(len as isize),
                                                                        stream.num_descriptors as
                                                                            libc::c_uint);
                            debug_assert_eq!(filled, stream.num_descriptors as libc::c_int);
                            run_context.descriptors.set_len(len + stream.num_descriptors);
                        }
                    }
                }

                let ret = libc::poll(run_context.descriptors.as_mut_ptr(),
                                     run_context.descriptors.len() as libc::nfds_t,
                                     -1 /* infinite */);
                assert!(ret >= 0, "poll() failed");

                if ret == 0 {
                    continue;
                }

                // If the `pending_trigger` was signaled, we need to process the comands.
                if run_context.descriptors[0].revents != 0 {
                    run_context.descriptors[0].revents = 0;
                    self.pending_trigger.clear_pipe();
                }

                // Iterate over each individual stream/descriptor.
                let mut i_stream = 0;
                let mut i_descriptor = 1;
                while (i_descriptor as usize) < run_context.descriptors.len() {
                    enum StreamType { Input, Output }
                    let stream_type;
                    let stream_inner = run_context.streams.get_mut(i_stream).unwrap();

                    // Check whether the event is `POLLOUT` or `POLLIN`. If neither, `continue`.
                    {
                        let mut revent = mem::uninitialized();

                        {
                            let num_descriptors = stream_inner.num_descriptors as libc::c_uint;
                            let desc_ptr =
                                run_context.descriptors.as_mut_ptr().offset(i_descriptor);
                            let res = alsa::snd_pcm_poll_descriptors_revents(stream_inner.channel,
                                                                             desc_ptr,
                                                                             num_descriptors,
                                                                             &mut revent);
                            check_errors(res).unwrap();
                        }

                        if revent as i16 == libc::POLLOUT {
                            stream_type = StreamType::Output;
                        } else if revent as i16 == libc::POLLIN {
                            stream_type = StreamType::Input;
                        } else {
                            i_descriptor += stream_inner.num_descriptors as isize;
                            i_stream += 1;
                            continue;
                        }
                    }

                    // Determine the number of samples that are available to read/write.
                    let available = {
                        let available = alsa::snd_pcm_avail(stream_inner.channel); // TODO: what about snd_pcm_avail_update?

                        if available == -32 {
                            // buffer underrun
                            stream_inner.buffer_len
                        } else if available < 0 {
                            check_errors(available as libc::c_int)
                                .expect("buffer is not available");
                            unreachable!()
                        } else {
                            (available * stream_inner.num_channels as alsa::snd_pcm_sframes_t) as
                                usize
                        }
                    };

                    if available < stream_inner.period_len {
                        i_descriptor += stream_inner.num_descriptors as isize;
                        i_stream += 1;
                        continue;
                    }

                    let stream_id = stream_inner.id.clone();

                    match stream_type {
                        StreamType::Input => {
                            // Simplify shared logic across the sample format branches.
                            macro_rules! read_buffer {
                                ($T:ty, $Variant:ident) => {{
                                    // The buffer to read into.
                                    let mut buffer: Vec<$T> = iter::repeat(mem::uninitialized())
                                        .take(available)
                                        .collect();
                                    let err = alsa::snd_pcm_readi(
                                        stream_inner.channel,
                                        buffer.as_mut_ptr() as *mut _,
                                        available as _,
                                    );
                                    check_errors(err as _).expect("snd_pcm_readi error");
                                    let input_buffer = InputBuffer {
                                        buffer: &buffer,
                                    };
                                    let buffer = UnknownTypeInputBuffer::$Variant(::InputBuffer {
                                        buffer: Some(input_buffer),
                                    });
                                    let stream_data = StreamData::Input { buffer: buffer };
                                    callback(stream_id, stream_data);
                                }};
                            }

                            match stream_inner.sample_format {
                                SampleFormat::I16 => read_buffer!(i16, I16),
                                SampleFormat::U16 => read_buffer!(u16, U16),
                                SampleFormat::F32 => read_buffer!(f32, F32),
                            }
                        },
                        StreamType::Output => {
                            // We're now sure that we're ready to write data.
                            let buffer = match stream_inner.sample_format {
                                SampleFormat::I16 => {
                                    let buffer = OutputBuffer {
                                        stream_inner: stream_inner,
                                        buffer: iter::repeat(mem::uninitialized())
                                            .take(available)
                                            .collect(),
                                    };

                                    UnknownTypeOutputBuffer::I16(::OutputBuffer { target: Some(buffer) })
                                },
                                SampleFormat::U16 => {
                                    let buffer = OutputBuffer {
                                        stream_inner: stream_inner,
                                        buffer: iter::repeat(mem::uninitialized())
                                            .take(available)
                                            .collect(),
                                    };

                                    UnknownTypeOutputBuffer::U16(::OutputBuffer { target: Some(buffer) })
                                },
                                SampleFormat::F32 => {
                                    let buffer = OutputBuffer {
                                        stream_inner: stream_inner,
                                        // Note that we don't use `mem::uninitialized` because of sNaN.
                                        buffer: iter::repeat(0.0).take(available).collect(),
                                    };

                                    UnknownTypeOutputBuffer::F32(::OutputBuffer { target: Some(buffer) })
                                },
                            };

                            let stream_data = StreamData::Output { buffer: buffer };
                            callback(stream_id, stream_data);
                        },
                    }
                }
            }
        }
    }

    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
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
                -16 /* determined empirically */ => return Err(CreationError::DeviceNotAvailable),
                e => check_errors(e).expect("Device unavailable")
            }
            let hw_params = HwParams::alloc();

            set_hw_params_from_format(capture_handle, &hw_params, format);

            let can_pause = alsa::snd_pcm_hw_params_can_pause(hw_params.0) == 1;

            let (buffer_len, period_len) = set_sw_params_from_format(capture_handle, format);

            check_errors(alsa::snd_pcm_prepare(capture_handle))
                .expect("could not get playback handle");

            let num_descriptors = {
                let num_descriptors = alsa::snd_pcm_poll_descriptors_count(capture_handle);
                debug_assert!(num_descriptors >= 1);
                num_descriptors as usize
            };

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            assert_ne!(new_stream_id.0, usize::max_value()); // check for overflows

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
            };

            check_errors(alsa::snd_pcm_start(capture_handle))
                .expect("could not start capture stream");

            self.push_command(Command::NewStream(stream_inner));
            Ok(new_stream_id)
        }
    }

    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
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
                -16 /* determined empirically */ => return Err(CreationError::DeviceNotAvailable),
                e => check_errors(e).expect("Device unavailable")
            }
            let hw_params = HwParams::alloc();

            set_hw_params_from_format(playback_handle, &hw_params, format);

            let can_pause = alsa::snd_pcm_hw_params_can_pause(hw_params.0) == 1;

            let (buffer_len, period_len) = set_sw_params_from_format(playback_handle, format);

            check_errors(alsa::snd_pcm_prepare(playback_handle))
                .expect("could not get playback handle");

            let num_descriptors = {
                let num_descriptors = alsa::snd_pcm_poll_descriptors_count(playback_handle);
                debug_assert!(num_descriptors >= 1);
                num_descriptors as usize
            };

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            assert_ne!(new_stream_id.0, usize::max_value()); // check for overflows

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
            };

            self.push_command(Command::NewStream(stream_inner));
            Ok(new_stream_id)
        }
    }

    #[inline]
    fn push_command(&self, command: Command) {
        self.commands.lock().unwrap().push(command);
        self.pending_trigger.wakeup();
    }

    #[inline]
    pub fn destroy_stream(&self, stream_id: StreamId) {
        self.push_command(Command::DestroyStream(stream_id));
    }

    #[inline]
    pub fn play_stream(&self, stream_id: StreamId) {
        self.push_command(Command::PlayStream(stream_id));
    }

    #[inline]
    pub fn pause_stream(&self, stream_id: StreamId) {
        self.push_command(Command::PauseStream(stream_id));
    }
}

unsafe fn set_hw_params_from_format(
    pcm_handle: *mut alsa::snd_pcm_t,
    hw_params: &HwParams,
    format: &Format,
) {
    check_errors(alsa::snd_pcm_hw_params_any(pcm_handle, hw_params.0))
        .expect("Errors on pcm handle");
    check_errors(alsa::snd_pcm_hw_params_set_access(pcm_handle,
                                                    hw_params.0,
                                                    alsa::SND_PCM_ACCESS_RW_INTERLEAVED))
        .expect("handle not acessible");

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

    check_errors(alsa::snd_pcm_hw_params_set_format(pcm_handle,
                                                    hw_params.0,
                                                    data_type))
        .expect("format could not be set");
    check_errors(alsa::snd_pcm_hw_params_set_rate(pcm_handle,
                                                  hw_params.0,
                                                  format.sample_rate.0 as libc::c_uint,
                                                  0))
        .expect("sample rate could not be set");
    check_errors(alsa::snd_pcm_hw_params_set_channels(pcm_handle,
                                                      hw_params.0,
                                                      format.channels as
                                                          libc::c_uint))
        .expect("channel count could not be set");
    let mut max_buffer_size = format.sample_rate.0 as alsa::snd_pcm_uframes_t /
        format.channels as alsa::snd_pcm_uframes_t /
        5; // 200ms of buffer
    check_errors(alsa::snd_pcm_hw_params_set_buffer_size_max(pcm_handle,
                                                             hw_params.0,
                                                             &mut max_buffer_size))
        .unwrap();
    check_errors(alsa::snd_pcm_hw_params(pcm_handle, hw_params.0))
        .expect("hardware params could not be set");
}

unsafe fn set_sw_params_from_format(
    pcm_handle: *mut alsa::snd_pcm_t,
    format: &Format,
) -> (usize, usize)
{
    let mut sw_params = mem::uninitialized(); // TODO: RAII
    check_errors(alsa::snd_pcm_sw_params_malloc(&mut sw_params)).unwrap();
    check_errors(alsa::snd_pcm_sw_params_current(pcm_handle, sw_params)).unwrap();
    check_errors(alsa::snd_pcm_sw_params_set_start_threshold(pcm_handle,
                                                             sw_params,
                                                             0))
        .unwrap();

    let (buffer_len, period_len) = {
        let mut buffer = mem::uninitialized();
        let mut period = mem::uninitialized();
        check_errors(alsa::snd_pcm_get_params(pcm_handle, &mut buffer, &mut period))
            .expect("could not initialize buffer");
        assert!(buffer != 0);
        check_errors(alsa::snd_pcm_sw_params_set_avail_min(pcm_handle,
                                                           sw_params,
                                                           period))
            .unwrap();
        let buffer = buffer as usize * format.channels as usize;
        let period = period as usize * format.channels as usize;
        (buffer, period)
    };

    check_errors(alsa::snd_pcm_sw_params(pcm_handle, sw_params)).unwrap();
    alsa::snd_pcm_sw_params_free(sw_params);
    (buffer_len, period_len)
}

pub struct InputBuffer<'a, T: 'a> {
    buffer: &'a [T],
}

pub struct OutputBuffer<'a, T: 'a> {
    stream_inner: &'a mut StreamInner,
    buffer: Vec<T>,
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

impl<'a, T> InputBuffer<'a, T> {
    #[inline]
    pub fn buffer(&self) -> &[T] {
        &self.buffer
    }

    #[inline]
    pub fn finish(self) {
        // Nothing to be done.
    }
}

impl<'a, T> OutputBuffer<'a, T> {
    #[inline]
    pub fn buffer(&mut self) -> &mut [T] {
        &mut self.buffer
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn finish(self) {
        let to_write = (self.buffer.len() / self.stream_inner.num_channels as usize) as
            alsa::snd_pcm_uframes_t;

        unsafe {
            loop {
                let result = alsa::snd_pcm_writei(self.stream_inner.channel,
                                                  self.buffer.as_ptr() as *const _,
                                                  to_write);

                if result == -32 {
                    // buffer underrun
                    alsa::snd_pcm_prepare(self.stream_inner.channel);
                } else if result < 0 {
                    check_errors(result as libc::c_int).expect("could not write pcm");
                } else {
                    assert_eq!(result as alsa::snd_pcm_uframes_t, to_write);
                    break;
                }
            }
        }
    }
}

#[inline]
fn check_errors(err: libc::c_int) -> Result<(), String> {
    use std::ffi;

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
