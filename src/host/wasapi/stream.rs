use super::check_result;
use super::com;
use super::winapi::shared::basetsd::UINT32;
use super::winapi::shared::ksmedia;
use super::winapi::shared::minwindef::{BYTE, DWORD, FALSE, WORD};
use super::winapi::shared::mmreg;
use super::winapi::um::audioclient::{self, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_S_BUFFER_EMPTY};
use super::winapi::um::handleapi;
use super::winapi::um::synchapi;
use super::winapi::um::winbase;
use super::winapi::um::winnt;

use std::mem;
use std::ptr;
use std::slice;
use std::sync::Mutex;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use std::sync::{Arc};

use BackendSpecificError;
use BuildStreamError;
use Format;
use PauseStreamError;
use PlayStreamError;
use SampleFormat;
use StreamData;
use StreamError;
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
    commands: Sender<Command>,

    // This event is signalled after a new entry is added to `commands`, so that the `run()`
    // method can be notified.
    pending_scheduled_event: winnt::HANDLE,
}

struct RunContext {
    // Streams that have been created in this event loop.
    stream: Arc<StreamInner>,

    // Handles corresponding to the `event` field of each element of `voices`. Must always be in
    // sync with `voices`, except that the first element is always `pending_scheduled_event`.
    handles: Vec<winnt::HANDLE>,

    commands: Receiver<Command>,
}

pub (crate) enum Command {
    PlayStream,
    PauseStream,
}

pub (crate) enum AudioClientFlow {
    Render {
        render_client: *mut audioclient::IAudioRenderClient,
    },
    Capture {
        capture_client: *mut audioclient::IAudioCaptureClient,
    },
}

pub (crate) struct StreamInner {
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

        let (tx, rx) = channel();

        EventLoop {
            pending_scheduled_event: pending_scheduled_event,
            run_context: Mutex::new(RunContext {
                                        stream: Arc::new(),
                                        handles: vec![pending_scheduled_event],
                                        commands: rx,
                                    }),
            next_stream_id: AtomicUsize::new(0),
            commands: tx,
        }
    }

    #[inline]
    pub(crate) fn destroy_stream(&self, stream_id: StreamId) {
        self.push_command(Command::DestroyStream(stream_id));
    }

    #[inline]
    pub(crate) fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData)
    {
        self.run_inner(&mut callback);
    }

    #[inline]
    pub(crate) fn play_stream(&self, stream: StreamId) -> Result<(), PlayStreamError> {
        self.push_command(Command::PlayStream(stream));
        Ok(())
    }

    #[inline]
    pub(crate) fn pause_stream(&self, stream: StreamId) -> Result<(), PauseStreamError> {
        self.push_command(Command::PauseStream(stream));
        Ok(())
    }

    #[inline]
    fn push_command(&self, command: Command) {
        // Safe to unwrap: sender outlives receiver.
        self.commands.send(command).unwrap();
        unsafe {
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

// Process any pending commands that are queued within the `RunContext`.
fn process_commands(run_context: &mut RunContext) -> Result<(), StreamError> {
    // Process the pending commands.
    for command in run_context.commands.try_iter() {
        match command {
            Command::PlayStream => {
                if !run_context.stream.playing {
                    let hresult = unsafe {
                        (*run_context.stream.audio_client).Start()
                    };

                    if let Err(err) = stream_error_from_hresult(hresult) {
                        return Err(err);
                    }
                    run_context.stream.playing = true;
                }
            },
            Command::PauseStream => {
                if run_context.stream.playing {
                    let hresult = unsafe {
                        (*run_context.stream.audio_client).Stop()
                    };
                    if let Err(err) = stream_error_from_hresult(hresult) {
                        return Err(err);
                    }
                    run_context.stream.playing = false;
                }
            },
        }
    }

    Ok(())
}
// Wait for any of the given handles to be signalled.
//
// Returns the index of the `handle` that was signalled, or an `Err` if
// `WaitForMultipleObjectsEx` fails.
//
// This is called when the `run` thread is ready to wait for the next event. The
// next event might be some command submitted by the user (the first handle) or
// might indicate that one of the streams is ready to deliver or receive audio.
fn wait_for_handle_signal(handles: &[winnt::HANDLE]) -> Result<usize, BackendSpecificError> {
    debug_assert!(handles.len() <= winnt::MAXIMUM_WAIT_OBJECTS as usize);
    let result = unsafe {
        synchapi::WaitForMultipleObjectsEx(
            handles.len() as u32,
            handles.as_ptr(),
            FALSE, // Don't wait for all, just wait for the first
            winbase::INFINITE, // TODO: allow setting a timeout
            FALSE, // irrelevant parameter here
        )
    };
    if result == winbase::WAIT_FAILED {
        let err = unsafe {
            winapi::um::errhandlingapi::GetLastError()
        };
        let description = format!("`WaitForMultipleObjectsEx failed: {}", err);
        let err = BackendSpecificError { description };
        return Err(err);
    }
    // Notifying the corresponding task handler.
    debug_assert!(result >= winbase::WAIT_OBJECT_0);
    let handle_idx = (result - winbase::WAIT_OBJECT_0) as usize;
    Ok(handle_idx)
}

// Get the number of available frames that are available for writing/reading.
fn get_available_frames(stream: &StreamInner) -> Result<u32, StreamError> {
    unsafe {
        let mut padding = mem::uninitialized();
        let hresult = (*stream.audio_client).GetCurrentPadding(&mut padding);
        stream_error_from_hresult(hresult)?;
        Ok(stream.max_frames_in_buffer - padding)
    }
}

// Convert the given `HRESULT` into a `StreamError` if it does indicate an error.
fn stream_error_from_hresult(hresult: winnt::HRESULT) -> Result<(), StreamError> {
    if hresult == AUDCLNT_E_DEVICE_INVALIDATED {
        return Err(StreamError::DeviceNotAvailable);
    }
    if let Err(err) = check_result(hresult) {
        let description = format!("{}", err);
        let err = BackendSpecificError { description };
        return Err(err.into());
    }
    Ok(())
}

fn run_inner(run_context: RunContext, data_callback: &mut dyn FnMut(StreamData), error_callback: &mut dyn FnMut(StreamError)) -> () {
    unsafe {
        'stream_loop: loop {
            // Process queued commands.
            match process_commands(run_context, error_callback) {
                Ok(()) => (),
                Err(err) => {
                    error_callback(err);
                    break 'stream_loop;
                }
            };

            // Wait for any of the handles to be signalled.
            let handle_idx = match wait_for_handle_signal(&run_context.handles) {
                Ok(idx) => idx,
                Err(err) => {
                    error_callback(err);
                    break 'stream_loop;
                }
            };

            // If `handle_idx` is 0, then it's `pending_scheduled_event` that was signalled in
            // order for us to pick up the pending commands. Otherwise, a stream needs data.
            if handle_idx == 0 {
                continue;
            }

            let stream = run_context.stream;
            let sample_size = stream.sample_format.sample_size();

            // Obtaining a pointer to the buffer.
            match stream.client_flow {

                AudioClientFlow::Capture { capture_client } => {
                    let mut frames_available = 0;
                    // Get the available data in the shared buffer.
                    let mut buffer: *mut BYTE = mem::uninitialized();
                    let mut flags = mem::uninitialized();
                    loop {
                        let hresult = (*capture_client).GetNextPacketSize(&mut frames_available);
                        if let Err(err) = stream_error_from_hresult(hresult) {
                            error_callback(err);
                            break 'stream_loop;
                        }
                        if frames_available == 0 {
                            break;
                        }
                        let hresult = (*capture_client).GetBuffer(
                            &mut buffer,
                            &mut frames_available,
                            &mut flags,
                            ptr::null_mut(),
                            ptr::null_mut(),
                        );

                        // TODO: Can this happen?
                        if hresult == AUDCLNT_S_BUFFER_EMPTY {
                            continue;
                        } else if let Err(err) = stream_error_from_hresult(hresult) {
                            error_callback(err);
                            break 'stream_loop;
                        }

                        debug_assert!(!buffer.is_null());

                        let buffer_len = frames_available as usize
                            * stream.bytes_per_frame as usize / sample_size;

                        // Simplify the capture callback sample format branches.
                        macro_rules! capture_callback {
                            ($T:ty, $Variant:ident) => {{
                                let buffer_data = buffer as *mut _ as *const $T;
                                let slice = slice::from_raw_parts(buffer_data, buffer_len);
                                let unknown_buffer = UnknownTypeInputBuffer::$Variant(::InputBuffer {
                                    buffer: slice,
                                });
                                let data = StreamData::Input { buffer: unknown_buffer };
                                data_callback(stream.id.clone(), Ok(data));
                                // Release the buffer.
                                let hresult = (*capture_client).ReleaseBuffer(frames_available);
                                if let Err(err) = stream_error_from_hresult(hresult) {
                                    error_callback(err);
                                    break 'stream_loop;
                                }
                            }};
                        }

                        match stream.sample_format {
                            SampleFormat::F32 => capture_callback!(f32, F32),
                            SampleFormat::I16 => capture_callback!(i16, I16),
                            SampleFormat::U16 => capture_callback!(u16, U16),
                        }
                    }
                },

                AudioClientFlow::Render { render_client } => {
                    // The number of frames available for writing.
                    let frames_available = match get_available_frames(stream) {
                        Ok(0) => continue, // TODO: Can this happen?
                        Ok(n) => n,
                        Err(err) => {
                            error_callback(err);
                            break 'stream_loop;
                        }
                    };

                    let mut buffer: *mut BYTE = mem::uninitialized();
                    let hresult = (*render_client).GetBuffer(
                        frames_available,
                        &mut buffer as *mut *mut _,
                    );

                    if let Err(err) = stream_error_from_hresult(hresult) {
                        error_callback(err);
                        break 'stream_loop;
                    }

                    debug_assert!(!buffer.is_null());
                    let buffer_len = frames_available as usize
                        * stream.bytes_per_frame as usize / sample_size;

                    // Simplify the render callback sample format branches.
                    macro_rules! render_callback {
                        ($T:ty, $Variant:ident) => {{
                            let buffer_data = buffer as *mut $T;
                            let slice = slice::from_raw_parts_mut(buffer_data, buffer_len);
                            let unknown_buffer = UnknownTypeOutputBuffer::$Variant(::OutputBuffer {
                                buffer: slice
                            });
                            let data = StreamData::Output { buffer: unknown_buffer };
                            data_callback(stream.id.clone(), Ok(data));
                            let hresult = (*render_client)
                                .ReleaseBuffer(frames_available as u32, 0);
                            if let Err(err) = stream_error_from_hresult(hresult) {
                                error_callback(err);
                                break 'stream_loop;
                            }
                        }}
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
