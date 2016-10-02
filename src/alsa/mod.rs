extern crate alsa_sys as alsa;
extern crate libc;

pub use self::enumerate::{EndpointsIterator, get_default_endpoint};

use ChannelPosition;
use CreationError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SamplesRate;
use UnknownTypeBuffer;

use std::{ffi, cmp, iter, mem, ptr};
use std::vec::IntoIter as VecIntoIter;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use futures::Poll;
use futures::task::Task;
use futures::task;
use futures::stream::Stream;
use futures::Async;

pub type SupportedFormatsIterator = VecIntoIter<Format>;

mod enumerate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        unsafe {
            let mut playback_handle = mem::uninitialized();
            let device_name = ffi::CString::new(self.0.clone()).expect("Unable to get device name");

            match alsa::snd_pcm_open(&mut playback_handle, device_name.as_ptr() as *const _,
                                     alsa::SND_PCM_STREAM_PLAYBACK, alsa::SND_PCM_NONBLOCK)
            {
                -2 |
                -16 /* determined empirically */ => return Err(FormatsEnumerationError::DeviceNotAvailable),
                e => check_errors(e).expect("device not available")
            }

            let hw_params = HwParams::alloc();
            match check_errors(alsa::snd_pcm_hw_params_any(playback_handle, hw_params.0)) {
                Err(_) => return Ok(Vec::new().into_iter()),
                Ok(_) => ()
            };

            // TODO: check endianess
            const FORMATS: [(SampleFormat, alsa::snd_pcm_format_t); 3] = [
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
                (SampleFormat::F32, alsa::SND_PCM_FORMAT_FLOAT_LE),
                /*SND_PCM_FORMAT_FLOAT_BE,
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
                SND_PCM_FORMAT_U18_3BE,*/
            ];

            let mut supported_formats = Vec::new();
            for &(sample_format, alsa_format) in FORMATS.iter() {
                if alsa::snd_pcm_hw_params_test_format(playback_handle, hw_params.0, alsa_format) == 0 {
                    supported_formats.push(sample_format);
                }
            }

            let mut min_rate = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_rate_min(hw_params.0, &mut min_rate, ptr::null_mut())).expect("unable to get minimum supported rete");
            let mut max_rate = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_rate_max(hw_params.0, &mut max_rate, ptr::null_mut())).expect("unable to get maximum supported rate");

            let samples_rates = if min_rate == max_rate {
                vec![min_rate]
            /*} else if alsa::snd_pcm_hw_params_test_rate(playback_handle, hw_params.0, min_rate + 1, 0) == 0 {
                (min_rate .. max_rate + 1).collect()*/      // TODO: code is correct but returns lots of stuff
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
                    if alsa::snd_pcm_hw_params_test_rate(playback_handle, hw_params.0, rate, 0) == 0 {
                        rates.push(rate);
                    }
                }

                /*if rates.len() == 0 {
                    (min_rate .. max_rate + 1).collect()
                } else {*/
                    rates    // TODO: code is correct but returns lots of stuff
                //}
            };

            let mut min_channels = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_channels_min(hw_params.0, &mut min_channels)).expect("unable to get minimum supported channel count");
            let mut max_channels = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_channels_max(hw_params.0, &mut max_channels)).expect("unable to get maximum supported channel count");
            let max_channels = cmp::min(max_channels, 32);      // TODO: limiting to 32 channels or too much stuff is returned
            let supported_channels = (min_channels .. max_channels + 1).filter_map(|num| {
                if alsa::snd_pcm_hw_params_test_channels(playback_handle, hw_params.0, num) == 0 {
                    Some([ChannelPosition::FrontLeft, ChannelPosition::FrontRight,
                          ChannelPosition::BackLeft, ChannelPosition::BackRight,
                          ChannelPosition::FrontCenter, ChannelPosition::LowFrequency]
                                  .iter().take(num as usize).cloned().collect::<Vec<_>>())
                } else {
                    None
                }
            }).collect::<Vec<_>>();

            let mut output = Vec::with_capacity(supported_formats.len() * supported_channels.len() *
                                                samples_rates.len());
            for &data_type in supported_formats.iter() {
                for channels in supported_channels.iter() {
                    for &rate in samples_rates.iter() {
                        output.push(Format {
                            channels: channels.clone(),
                            samples_rate: SamplesRate(rate as u32),
                            data_type: data_type,
                        });
                    }
                }
            }

            // TODO: RAII
            alsa::snd_pcm_close(playback_handle);
            Ok(output.into_iter())
        }
    }

    #[inline]
    pub fn get_name(&self) -> String {
        self.0.clone()
    }
}

pub struct EventLoop {
    inner: Arc<EventLoopInner>,
}

struct EventLoopInner {
    // Descriptors that we are currently waiting upon. This member is always locked while `run()`
    // is executed, ie. most of the time.
    //
    // Note that for `current_wait`, the first element of `descriptors` is always
    // `pending_wait_signal`. Therefore the length of `descriptors` is always one more than
    // `voices`.
    current_wait: Mutex<PollDescriptors>,

    // Since we can't add elements to `current_wait` (as it's locked), we add them to
    // `pending_wait`. Once that's done, we signal `pending_wait_signal` so that the `run()`
    // function can pause and add the content of `pending_wait` to `current_wait`.
    pending_wait: Mutex<PollDescriptors>,

    // A file descriptor opened with `eventfd`. Always the first element
    // of `current_wait.descriptors`. Should be notified when an element is added
    // to `pending_wait` so that the current wait can stop and take the pending wait into
    // account.
    pending_wait_signal: libc::c_int,
}

struct PollDescriptors {
    // Descriptors to wait for.
    descriptors: Vec<libc::pollfd>,
    // List of voices that are written in `descriptors`.
    voices: Vec<Arc<VoiceInner>>,
}

unsafe impl Send for EventLoopInner {}
unsafe impl Sync for EventLoopInner {}

impl Drop for EventLoopInner {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.pending_wait_signal);
        }
    }
}

impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        let pending_wait_signal = unsafe { libc::eventfd(0, 0) };

        EventLoop {
            inner: Arc::new(EventLoopInner {
                current_wait: Mutex::new(PollDescriptors {
                    descriptors: vec![libc::pollfd {
                        fd: pending_wait_signal,
                        events: libc::POLLIN,
                        revents: 0,
                    }],
                    voices: Vec::new(),
                }),
                pending_wait: Mutex::new(PollDescriptors {
                    descriptors: Vec::new(),
                    voices: Vec::new(),
                }),
                pending_wait_signal: pending_wait_signal,
            })
        }
    }

    #[inline]
    pub fn run(&self) {
        unsafe {
            let mut current_wait = self.inner.current_wait.lock().unwrap();

            loop {
                let ret = libc::poll(current_wait.descriptors.as_mut_ptr(),
                                     current_wait.descriptors.len() as libc::nfds_t,
                                     -1 /* infinite */);
                assert!(ret >= 0, "poll() failed");

                if ret == 0 {
                    continue;
                }

                // If the `pending_wait_signal` was signaled, add the pending waits to
                // the current waits.
                if current_wait.descriptors[0].revents != 0 {
                    current_wait.descriptors[0].revents = 0;

                    let mut pending = self.inner.pending_wait.lock().unwrap();
                    current_wait.descriptors.append(&mut pending.descriptors);
                    current_wait.voices.append(&mut pending.voices);

                    // Emptying the signal.
                    let mut out = 0u64;
                    let ret = libc::read(self.inner.pending_wait_signal,
                                         &mut out as *mut u64 as *mut _, 8);
                    assert_eq!(ret, 8);
                }

                // Check each individual descriptor for events.
                let mut i_voice = 0;
                let mut i_descriptor = 1;
                while i_voice < current_wait.voices.len() {
                    let kind = {
                        let scheduled = current_wait.voices[i_voice].scheduled.lock().unwrap();
                        match *scheduled {
                            Some(ref scheduled) => scheduled.kind,
                            None => panic!("current wait unscheduled task"),
                        }
                    };

                    // Depending on the kind of scheduling the number of descriptors corresponding
                    // to the voice and the events associated are different
                    match kind {
                        ScheduledKind::WaitPCM => {
                            let mut revent = mem::uninitialized();

                            {
                                let channel = *current_wait.voices[i_voice].channel.lock().unwrap();
                                let num_descriptors = current_wait.voices[i_voice].num_descriptors as libc::c_uint;
                                check_errors(alsa::snd_pcm_poll_descriptors_revents(channel, current_wait.descriptors
                                                                                    .as_mut_ptr().offset(i_descriptor),
                                                                                    num_descriptors, &mut revent)).unwrap();
                            }

                            if (revent as libc::c_short & libc::POLLOUT) != 0 {
                                let scheduled = current_wait.voices[i_voice].scheduled.lock().unwrap().take();
                                scheduled.unwrap().task.unpark();

                                for _ in 0 .. current_wait.voices[i_voice].num_descriptors {
                                    current_wait.descriptors.remove(i_descriptor as usize);
                                }
                                current_wait.voices.remove(i_voice);

                            } else {
                                i_descriptor += current_wait.voices[i_voice].num_descriptors as isize;
                                i_voice += 1;
                            }
                        },
                        ScheduledKind::WaitResume => {
                            if current_wait.descriptors[i_descriptor as usize].revents != 0 {
                                // Unpark the task
                                let scheduled = current_wait.voices[i_voice].scheduled.lock().unwrap().take();
                                scheduled.unwrap().task.unpark();

                                // Emptying the signal.
                                let mut out = 0u64;
                                let ret = libc::read(current_wait.descriptors[i_descriptor as usize].fd,
                                                     &mut out as *mut u64 as *mut _, 8);
                                assert_eq!(ret, 8);

                                // Remove from current waiting poll descriptors
                                current_wait.descriptors.remove(i_descriptor as usize);
                                current_wait.voices.remove(i_voice);
                            } else {
                                i_descriptor += 1;
                                i_voice += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct Voice {
    inner: Arc<VoiceInner>,
}

pub struct Buffer<T> {
    inner: Arc<VoiceInner>,
    buffer: Vec<T>,
}

pub struct SamplesStream {
    inner: Arc<VoiceInner>,
}

pub struct Scheduled {
    task: Task,
    kind: ScheduledKind,
}

#[derive(Clone,Copy)]
pub enum ScheduledKind {
    WaitResume,
    WaitPCM,
}

struct VoiceInner {
    // The event loop used to create the voice.
    event_loop: Arc<EventLoopInner>,

    // The ALSA channel.
    channel: Mutex<*mut alsa::snd_pcm_t>,

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

    // If `Some`, something previously called `schedule` on the stream.
    scheduled: Mutex<Option<Scheduled>>,

    // Wherease the sample stream is paused
    is_paused: Arc<AtomicBool>,

    // A file descriptor opened with `eventfd`.
    // It is used to wait for resume signal.
    resume_signal: libc::c_int,
}

unsafe impl Send for VoiceInner {}
unsafe impl Sync for VoiceInner {}

impl SamplesStream {
    #[inline]
    fn schedule(&mut self, kind: ScheduledKind) {
        unsafe {
            let channel = self.inner.channel.lock().unwrap();

            // We start by filling `scheduled`.
            *self.inner.scheduled.lock().unwrap() = Some(Scheduled {
                task: task::park(),
                kind: kind,
            });

            let mut pending_wait = self.inner.event_loop.pending_wait.lock().unwrap();
            match kind {
                ScheduledKind::WaitPCM => {
                    // In this function we turn the `snd_pcm_t` into a collection of file descriptors.
                    // And we add these descriptors to `event_loop.pending_wait.descriptors`.
                    pending_wait.descriptors.reserve(self.inner.num_descriptors);

                    let len = pending_wait.descriptors.len();
                    let filled = alsa::snd_pcm_poll_descriptors(*channel,
                                                                pending_wait.descriptors.as_mut_ptr()
                                                                .offset(len as isize),
                                                                self.inner.num_descriptors as libc::c_uint);
                    debug_assert_eq!(filled, self.inner.num_descriptors as libc::c_int);
                    pending_wait.descriptors.set_len(len + self.inner.num_descriptors);
                },
                ScheduledKind::WaitResume => {
                    // And we add the descriptor corresponding to the resume signal
                    // to `event_loop.pending_wait.descriptors`.
                    pending_wait.descriptors.push(libc::pollfd {
                        fd: self.inner.resume_signal,
                        events: libc::POLLIN,
                        revents: 0,
                    });
                }
            }

            // We also fill `voices`.
            pending_wait.voices.push(self.inner.clone());

            // Now that `pending_wait` received additional descriptors, we signal the event
            // so that our event loops can pick it up.
            drop(pending_wait);
            let buf = 1u64;
            let wret = libc::write(self.inner.event_loop.pending_wait_signal,
                                   &buf as *const u64 as *const _, 8);
            assert!(wret == 8);
        }
    }
}

impl Stream for SamplesStream {
    type Item = UnknownTypeBuffer;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // If paused then we schedule the task and return `NotReady`
        if self.inner.is_paused.load(Ordering::Relaxed) {
            self.schedule(ScheduledKind::WaitResume);
            return Ok(Async::NotReady);
        }

        // Determine the number of samples that are available to write.
        let available = {
            let channel = self.inner.channel.lock().expect("could not lock channel");
            let available = unsafe { alsa::snd_pcm_avail(*channel) };       // TODO: what about snd_pcm_avail_update?

            if available == -32 {
                // buffer underrun
                self.inner.buffer_len
            } else if available < 0 {
                check_errors(available as libc::c_int).expect("buffer is not available");
                unreachable!()
            } else {
                (available * self.inner.num_channels as alsa::snd_pcm_sframes_t) as usize
            }
        };

        // If we don't have one period ready, schedule the task and return `NotReady`.
        if available < self.inner.period_len {
            self.schedule(ScheduledKind::WaitPCM);
            return Ok(Async::NotReady);
        }

        // We now sure that we're ready to write data.
        match self.inner.sample_format {
            SampleFormat::I16 => {
                let buffer = Buffer {
                    buffer: iter::repeat(unsafe { mem::uninitialized() }).take(available).collect(),
                    inner: self.inner.clone(),
                };

                Ok(Async::Ready((Some(UnknownTypeBuffer::I16(::Buffer { target: Some(buffer) })))))
            },
            SampleFormat::U16 => {
                let buffer = Buffer {
                    buffer: iter::repeat(unsafe { mem::uninitialized() }).take(available).collect(),
                    inner: self.inner.clone(),
                };

                Ok(Async::Ready((Some(UnknownTypeBuffer::U16(::Buffer { target: Some(buffer) })))))
            },
            SampleFormat::F32 => {
                let buffer = Buffer {
                    buffer: iter::repeat(unsafe { mem::uninitialized() }).take(available).collect(),
                    inner: self.inner.clone(),
                };

                Ok(Async::Ready((Some(UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) })))))
            },
        }
    }
}

/// Wrapper around `hw_params`.
struct HwParams(*mut alsa::snd_pcm_hw_params_t);

impl HwParams {
    pub fn alloc() -> HwParams {
        unsafe {
            let mut hw_params = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_malloc(&mut hw_params)).expect("unable to get hardware parameters");
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

impl Voice {
    pub fn new(endpoint: &Endpoint, format: &Format, event_loop: &EventLoop)
               -> Result<(Voice, SamplesStream), CreationError>
    {
        unsafe {
            let name = ffi::CString::new(endpoint.0.clone()).expect("unable to clone endpoint");

            let mut playback_handle = mem::uninitialized();
            match alsa::snd_pcm_open(&mut playback_handle, name.as_ptr(),
                                     alsa::SND_PCM_STREAM_PLAYBACK, 0)
            {
                -16 /* determined empirically */ => return Err(CreationError::DeviceNotAvailable),
                e => check_errors(e).expect("Device unavailable")
            }

            // TODO: check endianess
            let data_type = match format.data_type {
                SampleFormat::I16 => alsa::SND_PCM_FORMAT_S16_LE,
                SampleFormat::U16 => alsa::SND_PCM_FORMAT_U16_LE,
                SampleFormat::F32 => alsa::SND_PCM_FORMAT_FLOAT_LE,
            };

            let hw_params = HwParams::alloc();
            check_errors(alsa::snd_pcm_hw_params_any(playback_handle, hw_params.0)).expect("Errors on playback handle");
            check_errors(alsa::snd_pcm_hw_params_set_access(playback_handle, hw_params.0, alsa::SND_PCM_ACCESS_RW_INTERLEAVED)).expect("handle not acessible");
            check_errors(alsa::snd_pcm_hw_params_set_format(playback_handle, hw_params.0, data_type)).expect("format could not be set");
            check_errors(alsa::snd_pcm_hw_params_set_rate(playback_handle, hw_params.0, format.samples_rate.0 as libc::c_uint, 0)).expect("sample rate could not be set");
            check_errors(alsa::snd_pcm_hw_params_set_channels(playback_handle, hw_params.0, format.channels.len() as libc::c_uint)).expect("channel count could not be set");
            let mut max_buffer_size = format.samples_rate.0 as alsa::snd_pcm_uframes_t / format.channels.len() as alsa::snd_pcm_uframes_t / 5;  // 200ms of buffer
            check_errors(alsa::snd_pcm_hw_params_set_buffer_size_max(playback_handle, hw_params.0, &mut max_buffer_size)).unwrap();
            check_errors(alsa::snd_pcm_hw_params(playback_handle, hw_params.0)).expect("hardware params could not be set");

            let mut sw_params = mem::uninitialized();       // TODO: RAII
            check_errors(alsa::snd_pcm_sw_params_malloc(&mut sw_params)).unwrap();
            check_errors(alsa::snd_pcm_sw_params_current(playback_handle, sw_params)).unwrap();
            check_errors(alsa::snd_pcm_sw_params_set_avail_min(playback_handle, sw_params, 4096)).unwrap();
            check_errors(alsa::snd_pcm_sw_params_set_start_threshold(playback_handle, sw_params, 0)).unwrap();
            check_errors(alsa::snd_pcm_sw_params(playback_handle, sw_params)).unwrap();

            check_errors(alsa::snd_pcm_prepare(playback_handle)).expect("could not get playback handle");

            let (buffer_len, period_len) = {
                let mut buffer = mem::uninitialized();
                let mut period = mem::uninitialized();
                check_errors(alsa::snd_pcm_get_params(playback_handle, &mut buffer, &mut period)).expect("could not initialize buffer");
                assert!(buffer != 0);
                let buffer = buffer as usize * format.channels.len();
                let period = period as usize * format.channels.len();
                (buffer, period)
            };

            let num_descriptors = {
                let num_descriptors = alsa::snd_pcm_poll_descriptors_count(playback_handle);
                debug_assert!(num_descriptors >= 1);
                num_descriptors as usize
            };

            // The voice is initialized as paused
            let is_paused = Arc::new(AtomicBool::new(true));

            let samples_stream_inner = Arc::new(VoiceInner {
                event_loop: event_loop.inner.clone(),
                channel: Mutex::new(playback_handle),
                sample_format: format.data_type,
                num_descriptors: num_descriptors,
                num_channels: format.channels.len() as u16,
                buffer_len: buffer_len,
                period_len: period_len,
                scheduled: Mutex::new(None),
                is_paused: is_paused.clone(),
                resume_signal: libc::eventfd(0, 0),
            });

            Ok((Voice {
                inner: samples_stream_inner.clone()
            }, SamplesStream {
                inner: samples_stream_inner
            }))
        }
    }

    #[inline]
    pub fn play(&mut self) {
        // If it was paused then we resume and signal
        // FIXME: the signal is send even if the event loop wasn't waiting for resume, is that an issue ?
        if self.inner.is_paused.swap(false, Ordering::Relaxed) {
            unsafe {
                let buf = 1u64;
                let wret = libc::write(self.inner.resume_signal,
                                       &buf as *const u64 as *const _, 8);
                assert!(wret == 8);
            }
        }
    }

    #[inline]
    pub fn pause(&mut self) {
        self.inner.is_paused.store(true, Ordering::Relaxed);
    }
}

impl Drop for VoiceInner {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_close(*self.channel.lock().expect("drop for voice"));
        }
    }
}

impl<T> Buffer<T> {
    #[inline]
    pub fn get_buffer(&mut self) -> &mut [T] {
        &mut self.buffer
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn finish(self) {
        let to_write = (self.buffer.len() / self.inner.num_channels as usize)
                       as alsa::snd_pcm_uframes_t;
        let channel = self.inner.channel.lock().expect("Buffer channel lock failed");

        unsafe {
            loop {
                let result = alsa::snd_pcm_writei(*channel,
                                                  self.buffer.as_ptr() as *const _,
                                                  to_write);

                if result == -32 {
                    // buffer underrun
                    alsa::snd_pcm_prepare(*channel);
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
            let s = ffi::CStr::from_ptr(alsa::snd_strerror(err)).to_bytes().to_vec();
            let s = String::from_utf8(s).expect("Streaming error occured");
            return Err(s);
        }
    }

    Ok(())
}
