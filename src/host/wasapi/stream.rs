use super::Device;
use super::check_result;
use super::com;
use super::winapi::shared::basetsd::UINT32;
use super::winapi::shared::ksmedia;
use super::winapi::shared::minwindef::{BYTE, DWORD, FALSE, WORD};
use super::winapi::shared::mmreg;
use super::winapi::um::audioclient::{self, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_S_BUFFER_EMPTY};
use super::winapi::um::audiosessiontypes::{AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK};
use super::winapi::um::mmdeviceapi::eRender;
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

    pub(crate) fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match device.build_audioclient() {
                Ok(client) => client,
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) =>
                    return Err(BuildStreamError::DeviceNotAvailable),
                Err(e) => {
                    let description = format!("{}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = format_to_waveformatextensible(format)
                    .ok_or(BuildStreamError::FormatNotSupported)?;
                let share_mode = AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(BuildStreamError::FormatNotSupported),
                    Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
                    _ => (),
                }

                // Support capturing output devices.
                let mut stream_flags: DWORD = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
                if device.data_flow() == eRender {
                    stream_flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
                }

                // finally initializing the audio client
                let hresult = (*audio_client).Initialize(
                    share_mode,
                    stream_flags,
                    0,
                    0,
                    &format_attempt.Format,
                    ptr::null(),
                );
                match check_result(hresult) {
                    Err(ref e)
                        if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) => {
                        (*audio_client).Release();
                        return Err(BuildStreamError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        let description = format!("{}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
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
                        return Err(BuildStreamError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        let description = format!("{}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
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
                    let description = format!("failed to create event");
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                if let Err(e) = check_result((*audio_client).SetEventHandle(event)) {
                    (*audio_client).Release();
                    let description = format!("failed to call SetEventHandle: {}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

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
                        return Err(BuildStreamError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        let description = format!("failed to build capture client: {}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    },
                    Ok(()) => (),
                };

                &mut *capture_client
            };

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            if new_stream_id.0 == usize::max_value() {
                return Err(BuildStreamError::StreamIdOverflow);
            }

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

                self.push_command(Command::NewStream(inner));
            };

            Ok(new_stream_id)
        }
    }

    pub(crate) fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match device.build_audioclient() {
                Ok(client) => client,
                Err(ref e) if e.raw_os_error() == Some(AUDCLNT_E_DEVICE_INVALIDATED) =>
                    return Err(BuildStreamError::DeviceNotAvailable),
                Err(e) => {
                    let description = format!("{}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = format_to_waveformatextensible(format)
                    .ok_or(BuildStreamError::FormatNotSupported)?;
                let share_mode = AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(BuildStreamError::FormatNotSupported),
                    Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
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
                        return Err(BuildStreamError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        let description = format!("{}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
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
                    let description = format!("failed to create event");
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                match check_result((*audio_client).SetEventHandle(event)) {
                    Err(e) => {
                        (*audio_client).Release();
                        let description = format!("failed to call SetEventHandle: {}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
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
                        return Err(BuildStreamError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        let description = format!("failed to obtain buffer size: {}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
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
                        return Err(BuildStreamError::DeviceNotAvailable);
                    },
                    Err(e) => {
                        (*audio_client).Release();
                        let description = format!("failed to build render client: {}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    },
                    Ok(()) => (),
                };

                &mut *render_client
            };

            let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
            if new_stream_id.0 == usize::max_value() {
                return Err(BuildStreamError::StreamIdOverflow);
            }

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

                self.push_command(Command::NewStream(inner));
            };

            Ok(new_stream_id)
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

// Process any pending commands that are queued within the `RunContext`.
fn process_commands(
    run_context: &mut RunContext,
    callback: &mut dyn FnMut(StreamId, StreamData),
) {
    // Process the pending commands.
    for command in run_context.commands.try_iter() {
        match command {
            Command::NewStream(stream_inner) => {
                let event = stream_inner.event;
                run_context.stream.push(stream_inner);
                run_context.handles.push(event);
            },
            Command::DestroyStream(stream_id) => {
                match run_context.stream.iter().position(|s| s.id == stream_id) {
                    None => continue,
                    Some(p) => {
                        run_context.handles.remove(p + 1);
                        run_context.stream.remove(p);
                    },
                }
            },
            Command::PlayStream(stream_id) => {
                match run_context.stream.iter().position(|s| s.id == stream_id) {
                    None => continue,
                    Some(p) => {
                        if !run_context.stream[p].playing {
                            let hresult = unsafe {
                                (*run_context.stream[p].audio_client).Start()
                            };
                            match stream_error_from_hresult(hresult) {
                                Ok(()) => {
                                    run_context.stream[p].playing = true;
                                }
                                Err(err) => {
                                    callback(stream_id, Err(err.into()));
                                    run_context.handles.remove(p + 1);
                                    run_context.stream.remove(p);
                                }
                            }
                        }
                    }
                }
            },
            Command::PauseStream(stream_id) => {
                match run_context.stream.iter().position(|s| s.id == stream_id) {
                    None => continue,
                    Some(p) => {
                        if run_context.stream[p].playing {
                            let hresult = unsafe {
                                (*run_context.stream[p].audio_client).Stop()
                            };
                            match stream_error_from_hresult(hresult) {
                                Ok(()) => {
                                    run_context.stream[p].playing = false;
                                }
                                Err(err) => {
                                    callback(stream_id, Err(err.into()));
                                    run_context.handles.remove(p + 1);
                                    run_context.stream.remove(p);
                                }
                            }
                        }
                    },
                }
            },
        }
    }
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
