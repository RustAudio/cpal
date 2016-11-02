use super::com;
use super::kernel32;
use super::ole32;
use super::winapi;
use super::Endpoint;
use super::check_result;

use std::slice;
use std::mem;
use std::ptr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;

use futures::Poll;
use futures::task::Task;
use futures::task;
use futures::stream::Stream;
use futures::Async;

use CreationError;
use ChannelPosition;
use Format;
use SampleFormat;
use UnknownTypeBuffer;

pub struct EventLoop {
    inner: Arc<EventLoopInner>,
}

unsafe impl Send for EventLoop {}
unsafe impl Sync for EventLoop {}

struct EventLoopInner {
    // List of handles that are currently being polled or that are going to be polled. This mutex
    // is locked for as long as the event loop is running.
    //
    // In the `EventLoopScheduled`, the first handle in the list of handles is always
    // `pending_scheduled_event`. This means that the length of `handles` is always 1 + the length
    // of `task_handles`.
    // FIXME: no way to remove elements from that list?
    scheduled: Mutex<EventLoopScheduled>,

    // Since the above mutex is locked most of the time, we add new handles to this list instead.
    // After a new element is added to this list, you should notify `pending_scheduled_event`
    // so that they get transferred to `scheduled`.
    //
    // The length of `handles` and `task_handles` should always be equal.
    pending_scheduled: Mutex<EventLoopScheduled>,

    // This event is signalled after elements have been added to `pending_scheduled` in order to
    // notify that they should be picked up.
    pending_scheduled_event: winapi::HANDLE,
}

struct EventLoopScheduled {
    // List of handles that correspond to voices.
    // They are linked to `task_handles`, but we store them separately in order to easily call
    // `WaitForMultipleObjectsEx` on the array without having to perform any conversion.
    handles: Vec<winapi::HANDLE>,

    // List of task handles corresponding to `handles`. The second element is used to signal
    // the voice that it has been signaled.
    task_handles: Vec<(Task, Arc<AtomicBool>)>,
}

impl EventLoop {
    pub fn new() -> EventLoop {
        let pending_scheduled_event = unsafe {
            kernel32::CreateEventA(ptr::null_mut(), 0, 0, ptr::null())
        };

        EventLoop {
            inner: Arc::new(EventLoopInner {
                pending_scheduled_event: pending_scheduled_event,
                scheduled: Mutex::new(EventLoopScheduled {
                    handles: vec![pending_scheduled_event],
                    task_handles: vec![],
                }),
                pending_scheduled: Mutex::new(EventLoopScheduled {
                    handles: vec![],
                    task_handles: vec![],
                })
            })
        }
    }

    pub fn run(&self) {
        unsafe {
            let mut scheduled = self.inner.scheduled.lock().unwrap();

            loop {
                debug_assert!(scheduled.handles.len() == 1 + scheduled.task_handles.len());

                // Creating a voice checks for the MAXIMUM_WAIT_OBJECTS limit.
                // FIXME: this is not the case ^
                debug_assert!(scheduled.handles.len() <= winapi::MAXIMUM_WAIT_OBJECTS as usize);

                // Wait for any of the handles to be signalled, which means that the corresponding
                // sound needs a buffer.
                let result = kernel32::WaitForMultipleObjectsEx(scheduled.handles.len() as u32,
                                                                scheduled.handles.as_ptr(),
                                                                winapi::FALSE, winapi::INFINITE, /* TODO: allow setting a timeout */
                                                                winapi::FALSE /* irrelevant parameter here */);

                // Notifying the corresponding task handler.
                assert!(result >= winapi::WAIT_OBJECT_0);
                let handle_id = (result - winapi::WAIT_OBJECT_0) as usize;

                if handle_id == 0 {
                    // The `pending_scheduled_event` handle has been notified, which means that we
                    // should pick up the content of `pending_scheduled`.
                    let mut pending = self.inner.pending_scheduled.lock().unwrap();
                    scheduled.handles.append(&mut pending.handles);
                    scheduled.task_handles.append(&mut pending.task_handles);

                } else {
                    scheduled.handles.remove(handle_id);
                    let (task_handle, ready) = scheduled.task_handles.remove(handle_id - 1);
                    ready.store(true, Ordering::Relaxed);
                    task_handle.unpark();
                }
            }
        }
    }
}

impl Drop for EventLoop {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            kernel32::CloseHandle(self.inner.pending_scheduled_event);
        }
    }
}

pub struct Voice {
    inner: Arc<Mutex<VoiceInner>>,
    playing: bool,
}

pub struct SamplesStream {
    event_loop: Arc<EventLoopInner>,
    inner: Arc<Mutex<VoiceInner>>,
    // The event that is signalled whenever a buffer is ready to be submitted to the voice.
    event: winapi::HANDLE,      // TODO: not deleted
    max_frames_in_buffer: winapi::UINT32,
    bytes_per_frame: winapi::WORD,
    ready: Arc<AtomicBool>,
}

unsafe impl Send for SamplesStream {}
unsafe impl Sync for SamplesStream {}

struct VoiceInner {
    audio_client: *mut winapi::IAudioClient,
    render_client: *mut winapi::IAudioRenderClient,
}

unsafe impl Send for Voice {}
unsafe impl Sync for Voice {}

impl Voice {
    pub fn new(end_point: &Endpoint, format: &Format, event_loop: &EventLoop)
               -> Result<(Voice, SamplesStream), CreationError>
    {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match end_point.build_audioclient() {
                Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    return Err(CreationError::DeviceNotAvailable),
                e => e.unwrap(),
            };

            // Computing the format and initializing the device.
            let format = {
                let format_attempt = try!(format_to_waveformatextensible(format));
                let share_mode = winapi::AUDCLNT_SHAREMODE_SHARED;

                // `IsFormatSupported` checks whether the format is supported and fills
                // a `WAVEFORMATEX`
                let mut dummy_fmt_ptr: *mut winapi::WAVEFORMATEX = mem::uninitialized();
                let hresult = (*audio_client).IsFormatSupported(share_mode, &format_attempt.Format,
                                                                &mut dummy_fmt_ptr);
                // we free that `WAVEFORMATEX` immediately after because we don't need it
                if !dummy_fmt_ptr.is_null() {
                    ole32::CoTaskMemFree(dummy_fmt_ptr as *mut _);
                }

                // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
                // has been found) but we also treat this as an error
                match (hresult, check_result(hresult)) {
                    (_, Err(ref e))
                            if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    (_, Err(e)) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    (winapi::S_FALSE, _) => {
                        (*audio_client).Release();
                        return Err(CreationError::FormatNotSupported);
                    },
                    (_, Ok(())) => (),
                };

                // finally initializing the audio client
                let hresult = (*audio_client).Initialize(share_mode,
                                                         winapi::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                                         0, 0, &format_attempt.Format, ptr::null());
                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    Ok(()) => (),
                };

                format_attempt.Format
            };

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event = kernel32::CreateEventA(ptr::null_mut(), 0, 0, ptr::null());
                if event == ptr::null_mut() {
                    (*audio_client).Release();
                    panic!("Failed to create event");
                }

                match check_result((*audio_client).SetEventHandle(event)) {
                    Err(_) => {
                        (*audio_client).Release();
                        panic!("Failed to call SetEventHandle")
                    },
                    Ok(_) => ()
                };

                event
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = {
                let mut max_frames_in_buffer = mem::uninitialized();
                let hresult = (*audio_client).GetBufferSize(&mut max_frames_in_buffer);

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    Ok(()) => (),
                };

                max_frames_in_buffer
            };

            // Building a `IAudioRenderClient` that will be used to fill the samples buffer.
            let render_client = {
                let mut render_client: *mut winapi::IAudioRenderClient = mem::uninitialized();
                let hresult = (*audio_client).GetService(&winapi::IID_IAudioRenderClient,
                                                         &mut render_client
                                                            as *mut *mut winapi::IAudioRenderClient
                                                            as *mut _);

                match check_result(hresult) {
                    Err(ref e) if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) =>
                    {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    Ok(()) => (),
                };

                &mut *render_client
            };

            // Everything went fine.
            let inner = Arc::new(Mutex::new(VoiceInner {
                audio_client: audio_client,
                render_client: render_client,
            }));

            let voice = Voice {
                inner: inner.clone(),
                playing: false,
            };

            let samples_stream = SamplesStream {
                event_loop: event_loop.inner.clone(),
                inner: inner,
                event: event,
                max_frames_in_buffer: max_frames_in_buffer,
                bytes_per_frame: format.nBlockAlign,
                ready: Arc::new(AtomicBool::new(false)),
            };

            Ok((voice, samples_stream))
        }
    }

    #[inline]
    pub fn play(&mut self) {
        if !self.playing {
            let mut inner = self.inner.lock().unwrap();

            unsafe {
                let hresult = (*inner.audio_client).Start();
                check_result(hresult).unwrap();
            }
        }

        self.playing = true;
    }

    #[inline]
    pub fn pause(&mut self) {
        if self.playing {
            let mut inner = self.inner.lock().unwrap();

            unsafe {
                let hresult = (*inner.audio_client).Stop();
                check_result(hresult).unwrap();
            }
        }

        self.playing = false;
    }
}

impl SamplesStream {
    #[inline]
    fn schedule(&mut self) {
        let mut pending = self.event_loop.pending_scheduled.lock().unwrap();
        pending.handles.push(self.event);
        pending.task_handles.push((task::park(), self.ready.clone()));
        drop(pending);

        let result = unsafe { kernel32::SetEvent(self.event_loop.pending_scheduled_event) };
        assert!(result != 0);
    }
}

impl Stream for SamplesStream {
    type Item = UnknownTypeBuffer;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        unsafe {
            if self.ready.swap(false, Ordering::Relaxed) == false {
                // Despite its name this function does not block, because we pass `0`.
                let result = kernel32::WaitForSingleObject(self.event, 0);

                // Park the task and returning if the event is not ready.
                match result {
                    winapi::WAIT_OBJECT_0 => (),
                    winapi::WAIT_TIMEOUT => {
                        self.schedule();
                        return Ok(Async::NotReady);
                    },
                    _ => unreachable!()
                };
            }

            // If we reach here, that means we're ready to accept new samples.

            let poll = {
                let mut inner = self.inner.lock().unwrap();

                // Obtaining the number of frames that are available to be written.
                let frames_available = {
                    let mut padding = mem::uninitialized();
                    let hresult = (*inner.audio_client).GetCurrentPadding(&mut padding);
                    check_result(hresult).unwrap();
                    self.max_frames_in_buffer - padding
                };

                if frames_available == 0 {
                    Ok(Async::NotReady)
                } else {

                    // Obtaining a pointer to the buffer.
                    let (buffer_data, buffer_len) = {
                        let mut buffer: *mut winapi::BYTE = mem::uninitialized();
                        let hresult = (*inner.render_client).GetBuffer(frames_available,
                                                                       &mut buffer as *mut *mut _);
                        check_result(hresult).unwrap();     // FIXME: can return `AUDCLNT_E_DEVICE_INVALIDATED`
                        debug_assert!(!buffer.is_null());

                        (buffer as *mut _,
                         frames_available as usize * self.bytes_per_frame as usize / mem::size_of::<f32>())     // FIXME: correct size
                    };

                    let buffer = Buffer {
                        voice: self.inner.clone(),
                        buffer_data: buffer_data,
                        buffer_len: buffer_len,
                        frames: frames_available,
                    };

                    Ok(Async::Ready(Some(UnknownTypeBuffer::F32(::Buffer { target: Some(buffer) }))))        // FIXME: not necessarily F32
                }
            };

            if let Ok(Async::NotReady) = poll {
                self.schedule();
            }

            poll
        }
    }
}

impl Drop for VoiceInner {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.render_client).Release();
            (*self.audio_client).Release();
        }
    }
}

pub struct Buffer<T> {
    voice: Arc<Mutex<VoiceInner>>,

    buffer_data: *mut T,
    buffer_len: usize,
    frames: winapi::UINT32,
}

unsafe impl<T> Send for Buffer<T> {}

impl<T> Buffer<T> {
    #[inline]
    pub fn buffer(&mut self) -> &mut [T] {
        unsafe {
            slice::from_raw_parts_mut(self.buffer_data, self.buffer_len)
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer_len
    }

    #[inline]
    pub fn finish(self) {
        unsafe {
            let mut inner = self.voice.lock().unwrap();
            let hresult = (*inner.render_client).ReleaseBuffer(self.frames as u32, 0);
            match check_result(hresult) {
                // ignoring the error that is produced if the device has been disconnected
                Err(ref e)
                        if e.raw_os_error() == Some(winapi::AUDCLNT_E_DEVICE_INVALIDATED) => (),
                e => e.unwrap(),
            };
        }
    }
}

fn format_to_waveformatextensible(format: &Format)
                                  -> Result<winapi::WAVEFORMATEXTENSIBLE, CreationError>
{
    Ok(winapi::WAVEFORMATEXTENSIBLE {
        Format: winapi::WAVEFORMATEX {
            wFormatTag: match format.data_type {
                SampleFormat::I16 => winapi::WAVE_FORMAT_PCM,
                SampleFormat::F32 => winapi::WAVE_FORMAT_EXTENSIBLE,
                SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
            },
            nChannels: format.channels.len() as winapi::WORD,
            nSamplesPerSec: format.samples_rate.0 as winapi::DWORD,
            nAvgBytesPerSec: format.channels.len() as winapi::DWORD *
                             format.samples_rate.0 as winapi::DWORD *
                             format.data_type.get_sample_size() as winapi::DWORD,
            nBlockAlign: format.channels.len() as winapi::WORD *
                         format.data_type.get_sample_size() as winapi::WORD,
            wBitsPerSample: 8 * format.data_type.get_sample_size() as winapi::WORD,
            cbSize: match format.data_type {
                SampleFormat::I16 => 0,
                SampleFormat::F32 => (mem::size_of::<winapi::WAVEFORMATEXTENSIBLE>() -
                                      mem::size_of::<winapi::WAVEFORMATEX>()) as winapi::WORD,
                SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
            },
        },
        Samples: 8 * format.data_type.get_sample_size() as winapi::WORD,
        dwChannelMask: {
            let mut mask = 0;
            for &channel in format.channels.iter() {
                let raw_value = match channel {
                    ChannelPosition::FrontLeft => winapi::SPEAKER_FRONT_LEFT,
                    ChannelPosition::FrontRight => winapi::SPEAKER_FRONT_RIGHT,
                    ChannelPosition::FrontCenter => winapi::SPEAKER_FRONT_CENTER,
                    ChannelPosition::LowFrequency => winapi::SPEAKER_LOW_FREQUENCY,
                    ChannelPosition::BackLeft => winapi::SPEAKER_BACK_LEFT,
                    ChannelPosition::BackRight => winapi::SPEAKER_BACK_RIGHT,
                    ChannelPosition::FrontLeftOfCenter => winapi::SPEAKER_FRONT_LEFT_OF_CENTER,
                    ChannelPosition::FrontRightOfCenter => winapi::SPEAKER_FRONT_RIGHT_OF_CENTER,
                    ChannelPosition::BackCenter => winapi::SPEAKER_BACK_CENTER,
                    ChannelPosition::SideLeft => winapi::SPEAKER_SIDE_LEFT,
                    ChannelPosition::SideRight => winapi::SPEAKER_SIDE_RIGHT,
                    ChannelPosition::TopCenter => winapi::SPEAKER_TOP_CENTER,
                    ChannelPosition::TopFrontLeft => winapi::SPEAKER_TOP_FRONT_LEFT,
                    ChannelPosition::TopFrontCenter => winapi::SPEAKER_TOP_FRONT_CENTER,
                    ChannelPosition::TopFrontRight => winapi::SPEAKER_TOP_FRONT_RIGHT,
                    ChannelPosition::TopBackLeft => winapi::SPEAKER_TOP_BACK_LEFT,
                    ChannelPosition::TopBackCenter => winapi::SPEAKER_TOP_BACK_CENTER,
                    ChannelPosition::TopBackRight => winapi::SPEAKER_TOP_BACK_RIGHT,
                };

                // channels must be in the right order
                if raw_value <= mask {
                    return Err(CreationError::FormatNotSupported);
                }

                mask = mask | raw_value;
            }

            mask
        },
        SubFormat: match format.data_type {
            SampleFormat::I16 => winapi::KSDATAFORMAT_SUBTYPE_PCM,
            SampleFormat::F32 => winapi::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
            SampleFormat::U16 => return Err(CreationError::FormatNotSupported),
        },
    })
}
