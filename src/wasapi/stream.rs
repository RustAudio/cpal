use super::Device;
use super::check_result;
use super::com;
use super::winapi::shared::basetsd::UINT32;
use super::winapi::shared::ksmedia;
use super::winapi::shared::minwindef::{BYTE, DWORD, FALSE, WORD};
use super::winapi::shared::mmreg;
use super::winapi::um::audioclient::{self, AUDCLNT_E_DEVICE_INVALIDATED};
use super::winapi::um::audiosessiontypes::{AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK};
use super::winapi::um::handleapi;
use super::winapi::um::synchapi;
use super::winapi::um::winbase;
use super::winapi::um::winnt;

use std::marker::PhantomData;
use std::mem;
use std::ptr;
use std::slice;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use CreationError;
use Format;
use SampleFormat;
use StreamData;
use UnknownTypeOutputBuffer;
use UnknownTypeInputBuffer;

pub struct EventLoop {
    // Data used by the `run()` function implementation. The mutex is kept lock permanently by
    // `run()`. This ensures that two `run()` invocations can't run at the same time, and also
    // means that we shouldn't try to lock this field from anywhere else but `run()`.
    run_context: Mutex<RunContext>,

    // Identifier of the next stream to create. Each new stream increases this counter. If the
    // counter overflows, there's a panic.
    // TODO: use AtomicU64 instead
    next_stream_id: AtomicUsize,

    // Commands processed by the `run()` method that is currently running.
    // `pending_scheduled_event` must be signalled whenever a command is added here, so that it
    // will get picked up.
    // TODO: use a lock-free container
    commands: Mutex<Vec<Command>>,

    // This event is signalled after a new entry is added to `commands`, so that the `run()`
    // method can be notified.
    pending_scheduled_event: winnt::HANDLE,
}

struct RunContext {
    // Streams that have been created in this event loop.
    streams: Vec<StreamInner>,

    // Handles corresponding to the `event` field of each element of `voices`. Must always be in
    // sync with `voices`, except that the first element is always `pending_scheduled_event`.
    handles: Vec<winnt::HANDLE>,
}

enum Command {
    NewStream(StreamInner),
    DestroyStream(StreamId),
    PlayStream(StreamId),
    PauseStream(StreamId),
}

enum AudioClientFlow {
    Render {
        render_client: *mut audioclient::IAudioRenderClient,
    },
    Capture {
        capture_client: *mut audioclient::IAudioCaptureClient,
    },
}

struct StreamInner {
    id: StreamId,
    audio_client: *mut audioclient::IAudioClient,
    client_flow: AudioClientFlow,
    // Event that is signalled by WASAPI whenever audio data must be written.
    event: winnt::HANDLE,
    // True if the stream is currently playing. False if paused.
    playing: bool,
    // Number of frames of audio data in the underlying buffer allocated by WASAPI.
    max_frames_in_buffer: UINT32,
    // Number of bytes that each frame occupies.
    bytes_per_frame: WORD,
    // The sample format with which the stream was created.
    sample_format: SampleFormat,
}

impl EventLoop {
    pub fn new() -> EventLoop {
        let pending_scheduled_event =
            unsafe { synchapi::CreateEventA(ptr::null_mut(), 0, 0, ptr::null()) };

        EventLoop {
            pending_scheduled_event: pending_scheduled_event,
            run_context: Mutex::new(RunContext {
                                        streams: Vec::new(),
                                        handles: vec![pending_scheduled_event],
                                    }),
            next_stream_id: AtomicUsize::new(0),
            commands: Mutex::new(Vec::new()),
        }
    }

    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match device.build_audioclient() {
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) =>
                    return Err(CreationError::DeviceNotAvailable),
                e => e.unwrap(),
            };

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = format_to_waveformatextensible(format)
                    .ok_or(CreationError::FormatNotSupported)?;
                let share_mode = AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(CreationError::FormatNotSupported),
                    Err(_) => return Err(CreationError::DeviceNotAvailable),
                    _ => (),
                }

                // finally initializing the audio client
                let hresult = (*audio_client).Initialize(
                    share_mode,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    0,
                    0,
                    &format_attempt.Format,
                    ptr::null(),
                );
                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
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

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = {
                let mut max_frames_in_buffer = mem::uninitialized();
                let hresult = (*audio_client).GetBufferSize(&mut max_frames_in_buffer);

                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
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

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event = synchapi::CreateEventA(ptr::null_mut(), 0, 0, ptr::null());
                if event == ptr::null_mut() {
                    (*audio_client).Release();
                    panic!("Failed to create event");
                }

                match check_result((*audio_client).SetEventHandle(event)) {
                    Err(_) => {
                        (*audio_client).Release();
                        panic!("Failed to call SetEventHandle")
                    },
                    Ok(_) => (),
                };

                event
            };

            // Building a `IAudioCaptureClient` that will be used to read captured samples.
            let capture_client = {
                let mut capture_client: *mut audioclient::IAudioCaptureClient = mem::uninitialized();
                let hresult = (*audio_client).GetService(
                    &audioclient::IID_IAudioCaptureClient,
                    &mut capture_client as *mut *mut audioclient::IAudioCaptureClient as *mut _,
                );

                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
                        (*audio_client).Release();
                        return Err(CreationError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        panic!("{:?}", e);
                    },
                    Ok(()) => (),
                };

                &mut *capture_client
            };

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            assert_ne!(new_stream_id.0, usize::max_value()); // check for overflows

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            {
                let client_flow = AudioClientFlow::Capture {
                    capture_client: capture_client,
                };
                let inner = StreamInner {
                    id: new_stream_id.clone(),
                    audio_client: audio_client,
                    client_flow: client_flow,
                    event: event,
                    playing: false,
                    max_frames_in_buffer: max_frames_in_buffer,
                    bytes_per_frame: waveformatex.nBlockAlign,
                    sample_format: format.data_type,
                };

                self.commands.lock().unwrap().push(Command::NewStream(inner));

                let result = synchapi::SetEvent(self.pending_scheduled_event);
                assert!(result != 0);
            };

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
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match device.build_audioclient() {
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) =>
                    return Err(CreationError::DeviceNotAvailable),
                e => e.unwrap(),
            };

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = format_to_waveformatextensible(format)
                    .ok_or(CreationError::FormatNotSupported)?;
                let share_mode = AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(CreationError::FormatNotSupported),
                    Err(_) => return Err(CreationError::DeviceNotAvailable),
                    _ => (),
                }

                // finally initializing the audio client
                let hresult = (*audio_client).Initialize(share_mode,
                                                         AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                                         0,
                                                         0,
                                                         &format_attempt.Format,
                                                         ptr::null());
                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
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
                let event = synchapi::CreateEventA(ptr::null_mut(), 0, 0, ptr::null());
                if event == ptr::null_mut() {
                    (*audio_client).Release();
                    panic!("Failed to create event");
                }

                match check_result((*audio_client).SetEventHandle(event)) {
                    Err(_) => {
                        (*audio_client).Release();
                        panic!("Failed to call SetEventHandle")
                    },
                    Ok(_) => (),
                };

                event
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = {
                let mut max_frames_in_buffer = mem::uninitialized();
                let hresult = (*audio_client).GetBufferSize(&mut max_frames_in_buffer);

                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
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
                let mut render_client: *mut audioclient::IAudioRenderClient = mem::uninitialized();
                let hresult = (*audio_client).GetService(&audioclient::IID_IAudioRenderClient,
                                                         &mut render_client as
                                                             *mut *mut audioclient::IAudioRenderClient as
                                                             *mut _);

                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
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

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            assert_ne!(new_stream_id.0, usize::max_value()); // check for overflows

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            {
                let client_flow = AudioClientFlow::Render {
                    render_client: render_client,
                };
                let inner = StreamInner {
                    id: new_stream_id.clone(),
                    audio_client: audio_client,
                    client_flow: client_flow,
                    event: event,
                    playing: false,
                    max_frames_in_buffer: max_frames_in_buffer,
                    bytes_per_frame: waveformatex.nBlockAlign,
                    sample_format: format.data_type,
                };

                self.commands.lock().unwrap().push(Command::NewStream(inner));

                let result = synchapi::SetEvent(self.pending_scheduled_event);
                assert!(result != 0);
            };

            Ok(new_stream_id)
        }
    }

    #[inline]
    pub fn destroy_stream(&self, stream_id: StreamId) {
        unsafe {
            self.commands
                .lock()
                .unwrap()
                .push(Command::DestroyStream(stream_id));
            let result = synchapi::SetEvent(self.pending_scheduled_event);
            assert!(result != 0);
        }
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData)
    {
        self.run_inner(&mut callback);
    }

    fn run_inner(&self, callback: &mut FnMut(StreamId, StreamData)) -> ! {
        unsafe {
            // We keep `run_context` locked forever, which guarantees that two invocations of
            // `run()` cannot run simultaneously.
            let mut run_context = self.run_context.lock().unwrap();

            loop {
                // Process the pending commands.
                let mut commands_lock = self.commands.lock().unwrap();
                for command in commands_lock.drain(..) {
                    match command {
                        Command::NewStream(stream_inner) => {
                            let event = stream_inner.event;
                            run_context.streams.push(stream_inner);
                            run_context.handles.push(event);
                        },
                        Command::DestroyStream(stream_id) => {
                            match run_context.streams.iter().position(|v| v.id == stream_id) {
                                None => continue,
                                Some(p) => {
                                    run_context.handles.remove(p + 1);
                                    run_context.streams.remove(p);
                                },
                            }
                        },
                        Command::PlayStream(stream_id) => {
                            if let Some(v) = run_context.streams.get_mut(stream_id.0) {
                                if !v.playing {
                                    let hresult = (*v.audio_client).Start();
                                    check_result(hresult).unwrap();
                                    v.playing = true;
                                }
                            }
                        },
                        Command::PauseStream(stream_id) => {
                            if let Some(v) = run_context.streams.get_mut(stream_id.0) {
                                if v.playing {
                                    let hresult = (*v.audio_client).Stop();
                                    check_result(hresult).unwrap();
                                    v.playing = false;
                                }
                            }
                        },
                    }
                }
                drop(commands_lock);

                // Wait for any of the handles to be signalled, which means that the corresponding
                // sound needs a buffer.
                debug_assert!(run_context.handles.len() <= winnt::MAXIMUM_WAIT_OBJECTS as usize);
                let result = synchapi::WaitForMultipleObjectsEx(run_context.handles.len() as u32,
                                                                run_context.handles.as_ptr(),
                                                                FALSE,
                                                                winbase::INFINITE, /* TODO: allow setting a timeout */
                                                                FALSE /* irrelevant parameter here */);

                // Notifying the corresponding task handler.
                debug_assert!(result >= winbase::WAIT_OBJECT_0);
                let handle_id = (result - winbase::WAIT_OBJECT_0) as usize;

                // If `handle_id` is 0, then it's `pending_scheduled_event` that was signalled in
                // order for us to pick up the pending commands.
                // Otherwise, a stream needs data.
                if handle_id >= 1 {
                    let stream = &mut run_context.streams[handle_id - 1];
                    let stream_id = stream.id.clone();

                    // Obtaining the number of frames that are available to be written.
                    let mut frames_available = {
                        let mut padding = mem::uninitialized();
                        let hresult = (*stream.audio_client).GetCurrentPadding(&mut padding);
                        check_result(hresult).unwrap();
                        stream.max_frames_in_buffer - padding
                    };

                    if frames_available == 0 {
                        // TODO: can this happen?
                        continue;
                    }

                    let sample_size = stream.sample_format.sample_size();

                    // Obtaining a pointer to the buffer.
                    match stream.client_flow {

                        AudioClientFlow::Capture { capture_client } => {
                            // Get the available data in the shared buffer.
                            let mut buffer: *mut BYTE = mem::uninitialized();
                            let mut flags = mem::uninitialized();
                            let hresult = (*capture_client).GetBuffer(
                               &mut buffer,
                               &mut frames_available,
                               &mut flags,
                               ptr::null_mut(),
                               ptr::null_mut(),
                            );
                            check_result(hresult).unwrap();
                            debug_assert!(!buffer.is_null());
                            let buffer_len = frames_available as usize 
                                * stream.bytes_per_frame as usize / sample_size;

                            // Simplify the capture callback sample format branches.
                            macro_rules! capture_callback {
                                ($T:ty, $Variant:ident) => {{
                                    let buffer_data = buffer as *mut _ as *const $T;
                                    let slice = slice::from_raw_parts(buffer_data, buffer_len);
                                    let input_buffer = InputBuffer { buffer: slice };
                                    let unknown_buffer = UnknownTypeInputBuffer::$Variant(::InputBuffer {
                                        buffer: Some(input_buffer),
                                    });
                                    let data = StreamData::Input { buffer: unknown_buffer };
                                    callback(stream_id, data);

                                    // Release the buffer.
                                    let hresult = (*capture_client).ReleaseBuffer(frames_available);
                                    match check_result(hresult) {
                                        // Ignoring unavailable device error.
                                        Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
                                        },
                                        e => e.unwrap(),
                                    };
                                }};
                            }

                            match stream.sample_format {
                                SampleFormat::F32 => capture_callback!(f32, F32),
                                SampleFormat::I16 => capture_callback!(i16, I16),
                                SampleFormat::U16 => capture_callback!(u16, U16),
                            }
                        },

                        AudioClientFlow::Render { render_client } => {
                            let mut buffer: *mut BYTE = mem::uninitialized();
                            let hresult = (*render_client).GetBuffer(
                                frames_available,
                                &mut buffer as *mut *mut _,
                            );
                            // FIXME: can return `AUDCLNT_E_DEVICE_INVALIDATED`
                            check_result(hresult).unwrap(); 
                            debug_assert!(!buffer.is_null());
                            let buffer_len = frames_available as usize 
                                * stream.bytes_per_frame as usize / sample_size;

                            // Simplify the render callback sample format branches.
                            macro_rules! render_callback {
                                ($T:ty, $Variant:ident) => {{
                                    let buffer_data = buffer as *mut $T;
                                    let output_buffer = OutputBuffer {
                                        stream: stream,
                                        buffer_data: buffer_data,
                                        buffer_len: buffer_len,
                                        frames: frames_available,
                                        marker: PhantomData,
                                    };
                                    let unknown_buffer = UnknownTypeOutputBuffer::$Variant(::OutputBuffer {
                                        target: Some(output_buffer)
                                    });
                                    let data = StreamData::Output { buffer: unknown_buffer };
                                    callback(stream_id, data);
                                }};
                            }

                            match stream.sample_format {
                                SampleFormat::F32 => render_callback!(f32, F32),
                                SampleFormat::I16 => render_callback!(i16, I16),
                                SampleFormat::U16 => render_callback!(u16, U16),
                            }
                        },
                    }
                }
            }
        }
    }

    #[inline]
    pub fn play_stream(&self, stream: StreamId) {
        unsafe {
            self.commands.lock().unwrap().push(Command::PlayStream(stream));
            let result = synchapi::SetEvent(self.pending_scheduled_event);
            assert!(result != 0);
        }
    }

    #[inline]
    pub fn pause_stream(&self, stream: StreamId) {
        unsafe {
            self.commands.lock().unwrap().push(Command::PauseStream(stream));
            let result = synchapi::SetEvent(self.pending_scheduled_event);
            assert!(result != 0);
        }
    }
}

impl Drop for EventLoop {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            handleapi::CloseHandle(self.pending_scheduled_event);
        }
    }
}

unsafe impl Send for EventLoop {
}
unsafe impl Sync for EventLoop {
}

// The content of a stream ID is a number that was fetched from `next_stream_id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

impl Drop for AudioClientFlow {
    fn drop(&mut self) {
        unsafe {
            match *self {
                AudioClientFlow::Capture { capture_client } => (*capture_client).Release(),
                AudioClientFlow::Render { render_client } => (*render_client).Release(),
            };
        }
    }
}

impl Drop for StreamInner {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.audio_client).Release();
            handleapi::CloseHandle(self.event);
        }
    }
}

pub struct InputBuffer<'a, T: 'a> {
    buffer: &'a [T],
}

pub struct OutputBuffer<'a, T: 'a> {
    stream: &'a mut StreamInner,

    buffer_data: *mut T,
    buffer_len: usize,
    frames: UINT32,

    marker: PhantomData<&'a mut [T]>,
}

unsafe impl<'a, T> Send for OutputBuffer<'a, T> {
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
        unsafe { slice::from_raw_parts_mut(self.buffer_data, self.buffer_len) }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer_len
    }

    #[inline]
    pub fn finish(self) {
        unsafe {
            let hresult = match self.stream.client_flow {
                AudioClientFlow::Render { render_client } => {
                    (*render_client).ReleaseBuffer(self.frames as u32, 0)
                },
                _ => unreachable!(),
            };
            match check_result(hresult) {
                // Ignoring the error that is produced if the device has been disconnected.
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => (),
                e => e.unwrap(),
            };
        }
    }
}

// Turns a `Format` into a `WAVEFORMATEXTENSIBLE`.
//
// Returns `None` if the WAVEFORMATEXTENSIBLE does not support the given format.
fn format_to_waveformatextensible(format: &Format) -> Option<mmreg::WAVEFORMATEXTENSIBLE> {
    let format_tag = match format.data_type {
        SampleFormat::I16 => mmreg::WAVE_FORMAT_PCM,
        SampleFormat::F32 => mmreg::WAVE_FORMAT_EXTENSIBLE,
        SampleFormat::U16 => return None,
    };
    let channels = format.channels as WORD;
    let sample_rate = format.sample_rate.0 as DWORD;
    let sample_bytes = format.data_type.sample_size() as WORD;
    let avg_bytes_per_sec = channels as DWORD * sample_rate * sample_bytes as DWORD;
    let block_align = channels * sample_bytes;
    let bits_per_sample = 8 * sample_bytes;
    let cb_size = match format.data_type {
        SampleFormat::I16 => 0,
        SampleFormat::F32 => {
            let extensible_size = mem::size_of::<mmreg::WAVEFORMATEXTENSIBLE>();
            let ex_size = mem::size_of::<mmreg::WAVEFORMATEX>();
            (extensible_size - ex_size) as WORD
        },
        SampleFormat::U16 => return None,
    };
    let waveformatex = mmreg::WAVEFORMATEX {
        wFormatTag: format_tag,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: bits_per_sample,
        cbSize: cb_size,
    };

    // CPAL does not care about speaker positions, so pass audio straight through.
    // TODO: This constant should be defined in winapi but is missing.
    const KSAUDIO_SPEAKER_DIRECTOUT: DWORD = 0;
    let channel_mask = KSAUDIO_SPEAKER_DIRECTOUT;

    let sub_format = match format.data_type {
        SampleFormat::I16 => ksmedia::KSDATAFORMAT_SUBTYPE_PCM,
        SampleFormat::F32 => ksmedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        SampleFormat::U16 => return None,
    };
    let waveformatextensible = mmreg::WAVEFORMATEXTENSIBLE {
        Format: waveformatex,
        Samples: bits_per_sample as WORD,
        dwChannelMask: channel_mask,
        SubFormat: sub_format,
    };

    Some(waveformatextensible)
}
