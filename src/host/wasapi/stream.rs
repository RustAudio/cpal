use super::check_result;
use super::winapi::shared::basetsd::{UINT32, UINT64};
use super::winapi::shared::minwindef::{BYTE, FALSE, WORD};
use super::winapi::um::audioclient::{self, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_S_BUFFER_EMPTY};
use super::winapi::um::handleapi;
use super::winapi::um::synchapi;
use super::winapi::um::winbase;
use super::winapi::um::winnt;
use crate::traits::StreamTrait;
use crate::{
    BackendSpecificError, Data, InputCallbackInfo, OutputCallbackInfo, PauseStreamError,
    PlayStreamError, SampleFormat, StreamError,
};
use std::mem;
use std::ptr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{self, JoinHandle};

pub struct Stream {
    /// The high-priority audio processing thread calling callbacks.
    /// Option used for moving out in destructor.
    ///
    /// TODO: Actually set the thread priority.
    thread: Option<JoinHandle<()>>,

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
    stream: StreamInner,

    // Handles corresponding to the `event` field of each element of `voices`. Must always be in
    // sync with `voices`, except that the first element is always `pending_scheduled_event`.
    handles: Vec<winnt::HANDLE>,

    commands: Receiver<Command>,
}

// Once we start running the eventloop, the RunContext will not be moved.
unsafe impl Send for RunContext {}

pub enum Command {
    PlayStream,
    PauseStream,
    Terminate,
}

pub enum AudioClientFlow {
    Render {
        render_client: *mut audioclient::IAudioRenderClient,
    },
    Capture {
        capture_client: *mut audioclient::IAudioCaptureClient,
    },
}

pub struct StreamInner {
    pub audio_client: *mut audioclient::IAudioClient,
    pub audio_clock: *mut audioclient::IAudioClock,
    pub client_flow: AudioClientFlow,
    // Event that is signalled by WASAPI whenever audio data must be written.
    pub event: winnt::HANDLE,
    // True if the stream is currently playing. False if paused.
    pub playing: bool,
    // Number of frames of audio data in the underlying buffer allocated by WASAPI.
    pub max_frames_in_buffer: UINT32,
    // Number of bytes that each frame occupies.
    pub bytes_per_frame: WORD,
    // The configuration with which the stream was created.
    pub config: crate::StreamConfig,
    // The sample format with which the stream was created.
    pub sample_format: SampleFormat,
}

impl Stream {
    pub(crate) fn new_input<D, E>(
        stream_inner: StreamInner,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Stream
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let pending_scheduled_event =
            unsafe { synchapi::CreateEventA(ptr::null_mut(), 0, 0, ptr::null()) };
        let (tx, rx) = channel();

        let run_context = RunContext {
            handles: vec![pending_scheduled_event, stream_inner.event],
            stream: stream_inner,
            commands: rx,
        };

        let thread = thread::Builder::new()
            .name("cpal_wasapi_in".to_owned())
            .spawn(move || run_input(run_context, &mut data_callback, &mut error_callback))
            .unwrap();

        Stream {
            thread: Some(thread),
            commands: tx,
            pending_scheduled_event,
        }
    }

    pub(crate) fn new_output<D, E>(
        stream_inner: StreamInner,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Stream
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let pending_scheduled_event =
            unsafe { synchapi::CreateEventA(ptr::null_mut(), 0, 0, ptr::null()) };
        let (tx, rx) = channel();

        let run_context = RunContext {
            handles: vec![pending_scheduled_event, stream_inner.event],
            stream: stream_inner,
            commands: rx,
        };

        let thread = thread::Builder::new()
            .name("cpal_wasapi_out".to_owned())
            .spawn(move || run_output(run_context, &mut data_callback, &mut error_callback))
            .unwrap();

        Stream {
            thread: Some(thread),
            commands: tx,
            pending_scheduled_event,
        }
    }

    #[inline]
    fn push_command(&self, command: Command) {
        // Sender generally outlives receiver, unless the device gets unplugged.
        let _ = self.commands.send(command);
        unsafe {
            let result = synchapi::SetEvent(self.pending_scheduled_event);
            assert_ne!(result, 0);
        }
    }
}

impl Drop for Stream {
    #[inline]
    fn drop(&mut self) {
        self.push_command(Command::Terminate);
        self.thread.take().unwrap().join().unwrap();
        unsafe {
            handleapi::CloseHandle(self.pending_scheduled_event);
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.push_command(Command::PlayStream);
        Ok(())
    }
    fn pause(&self) -> Result<(), PauseStreamError> {
        self.push_command(Command::PauseStream);
        Ok(())
    }
}

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
            (*self.audio_clock).Release();
            handleapi::CloseHandle(self.event);
        }
    }
}

// Process any pending commands that are queued within the `RunContext`.
// Returns `true` if the loop should continue running, `false` if it should terminate.
fn process_commands(run_context: &mut RunContext) -> Result<bool, StreamError> {
    // Process the pending commands.
    for command in run_context.commands.try_iter() {
        match command {
            Command::PlayStream => {
                if !run_context.stream.playing {
                    let hresult = unsafe { (*run_context.stream.audio_client).Start() };

                    if let Err(err) = stream_error_from_hresult(hresult) {
                        return Err(err);
                    }
                    run_context.stream.playing = true;
                }
            }
            Command::PauseStream => {
                if run_context.stream.playing {
                    let hresult = unsafe { (*run_context.stream.audio_client).Stop() };
                    if let Err(err) = stream_error_from_hresult(hresult) {
                        return Err(err);
                    }
                    run_context.stream.playing = false;
                }
            }
            Command::Terminate => {
                return Ok(false);
            }
        }
    }

    Ok(true)
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
            FALSE,             // Don't wait for all, just wait for the first
            winbase::INFINITE, // TODO: allow setting a timeout
            FALSE,             // irrelevant parameter here
        )
    };
    if result == winbase::WAIT_FAILED {
        let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
        let description = format!("`WaitForMultipleObjectsEx failed: {}", err);
        let err = BackendSpecificError { description };
        return Err(err);
    }
    // Notifying the corresponding task handler.
    let handle_idx = (result - winbase::WAIT_OBJECT_0) as usize;
    Ok(handle_idx)
}

// Get the number of available frames that are available for writing/reading.
fn get_available_frames(stream: &StreamInner) -> Result<u32, StreamError> {
    unsafe {
        let mut padding = 0u32;
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

fn run_input(
    mut run_ctxt: RunContext,
    data_callback: &mut dyn FnMut(&Data, &InputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) {
    loop {
        match process_commands_and_await_signal(&mut run_ctxt, error_callback) {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => continue,
            None => (),
        }
        let capture_client = match run_ctxt.stream.client_flow {
            AudioClientFlow::Capture { capture_client } => capture_client,
            _ => unreachable!(),
        };
        match process_input(
            &mut run_ctxt.stream,
            capture_client,
            data_callback,
            error_callback,
        ) {
            ControlFlow::Break => break,
            ControlFlow::Continue => continue,
        }
    }
}

fn run_output(
    mut run_ctxt: RunContext,
    data_callback: &mut dyn FnMut(&mut Data, &OutputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) {
    loop {
        match process_commands_and_await_signal(&mut run_ctxt, error_callback) {
            Some(ControlFlow::Break) => break,
            Some(ControlFlow::Continue) => continue,
            None => (),
        }
        let render_client = match run_ctxt.stream.client_flow {
            AudioClientFlow::Render { render_client } => render_client,
            _ => unreachable!(),
        };
        match process_output(
            &mut run_ctxt.stream,
            render_client,
            data_callback,
            error_callback,
        ) {
            ControlFlow::Break => break,
            ControlFlow::Continue => continue,
        }
    }
}

enum ControlFlow {
    Break,
    Continue,
}

fn process_commands_and_await_signal(
    run_context: &mut RunContext,
    error_callback: &mut dyn FnMut(StreamError),
) -> Option<ControlFlow> {
    // Process queued commands.
    match process_commands(run_context) {
        Ok(true) => (),
        Ok(false) => return Some(ControlFlow::Break),
        Err(err) => {
            error_callback(err);
            return Some(ControlFlow::Break);
        }
    };

    // Wait for any of the handles to be signalled.
    let handle_idx = match wait_for_handle_signal(&run_context.handles) {
        Ok(idx) => idx,
        Err(err) => {
            error_callback(err.into());
            return Some(ControlFlow::Break);
        }
    };

    // If `handle_idx` is 0, then it's `pending_scheduled_event` that was signalled in
    // order for us to pick up the pending commands. Otherwise, a stream needs data.
    if handle_idx == 0 {
        return Some(ControlFlow::Continue);
    }

    None
}

// The loop for processing pending input data.
fn process_input(
    stream: &StreamInner,
    capture_client: *mut audioclient::IAudioCaptureClient,
    data_callback: &mut dyn FnMut(&Data, &InputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) -> ControlFlow {
    let mut frames_available = 0;
    unsafe {
        // Get the available data in the shared buffer.
        let mut buffer: *mut BYTE = ptr::null_mut();
        let mut flags = mem::MaybeUninit::uninit();
        loop {
            let hresult = (*capture_client).GetNextPacketSize(&mut frames_available);
            if let Err(err) = stream_error_from_hresult(hresult) {
                error_callback(err);
                return ControlFlow::Break;
            }
            if frames_available == 0 {
                return ControlFlow::Continue;
            }
            let mut qpc_position: UINT64 = 0;
            let hresult = (*capture_client).GetBuffer(
                &mut buffer,
                &mut frames_available,
                flags.as_mut_ptr(),
                ptr::null_mut(),
                &mut qpc_position,
            );

            // TODO: Can this happen?
            if hresult == AUDCLNT_S_BUFFER_EMPTY {
                continue;
            } else if let Err(err) = stream_error_from_hresult(hresult) {
                error_callback(err);
                return ControlFlow::Break;
            }

            debug_assert!(!buffer.is_null());

            let data = buffer as *mut ();
            let len = frames_available as usize * stream.bytes_per_frame as usize
                / stream.sample_format.sample_size();
            let data = Data::from_parts(data, len, stream.sample_format);

            // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
            let timestamp = match input_timestamp(stream, qpc_position) {
                Ok(ts) => ts,
                Err(err) => {
                    error_callback(err);
                    return ControlFlow::Break;
                }
            };
            let info = InputCallbackInfo { timestamp };
            data_callback(&data, &info);

            // Release the buffer.
            let hresult = (*capture_client).ReleaseBuffer(frames_available);
            if let Err(err) = stream_error_from_hresult(hresult) {
                error_callback(err);
                return ControlFlow::Break;
            }
        }
    }
}

// The loop for writing output data.
fn process_output(
    stream: &StreamInner,
    render_client: *mut audioclient::IAudioRenderClient,
    data_callback: &mut dyn FnMut(&mut Data, &OutputCallbackInfo),
    error_callback: &mut dyn FnMut(StreamError),
) -> ControlFlow {
    // The number of frames available for writing.
    let frames_available = match get_available_frames(&stream) {
        Ok(0) => return ControlFlow::Continue, // TODO: Can this happen?
        Ok(n) => n,
        Err(err) => {
            error_callback(err);
            return ControlFlow::Break;
        }
    };

    unsafe {
        let mut buffer: *mut BYTE = ptr::null_mut();
        let hresult = (*render_client).GetBuffer(frames_available, &mut buffer as *mut *mut _);

        if let Err(err) = stream_error_from_hresult(hresult) {
            error_callback(err);
            return ControlFlow::Break;
        }

        debug_assert!(!buffer.is_null());

        let data = buffer as *mut ();
        let len = frames_available as usize * stream.bytes_per_frame as usize
            / stream.sample_format.sample_size();
        let mut data = Data::from_parts(data, len, stream.sample_format);
        let sample_rate = stream.config.sample_rate;
        let timestamp = match output_timestamp(stream, frames_available, sample_rate) {
            Ok(ts) => ts,
            Err(err) => {
                error_callback(err);
                return ControlFlow::Break;
            }
        };
        let info = OutputCallbackInfo { timestamp };
        data_callback(&mut data, &info);

        let hresult = (*render_client).ReleaseBuffer(frames_available as u32, 0);
        if let Err(err) = stream_error_from_hresult(hresult) {
            error_callback(err);
            return ControlFlow::Break;
        }
    }

    ControlFlow::Continue
}

/// Convert the given duration in frames at the given sample rate to a `std::time::Duration`.
fn frames_to_duration(frames: u32, rate: crate::SampleRate) -> std::time::Duration {
    let secsf = frames as f64 / rate.0 as f64;
    let secs = secsf as u64;
    let nanos = ((secsf - secs as f64) * 1_000_000_000.0) as u32;
    std::time::Duration::new(secs, nanos)
}

/// Use the stream's `IAudioClock` to produce the current stream instant.
///
/// Uses the QPC position produced via the `GetPosition` method.
fn stream_instant(stream: &StreamInner) -> Result<crate::StreamInstant, StreamError> {
    let mut position: UINT64 = 0;
    let mut qpc_position: UINT64 = 0;
    let res = unsafe { (*stream.audio_clock).GetPosition(&mut position, &mut qpc_position) };
    stream_error_from_hresult(res)?;
    // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
    let qpc_nanos = qpc_position as i128 * 100;
    let instant = crate::StreamInstant::from_nanos_i128(qpc_nanos)
        .expect("performance counter out of range of `StreamInstant` representation");
    Ok(instant)
}

/// Produce the input stream timestamp.
///
/// `buffer_qpc_position` is the `qpc_position` returned via the `GetBuffer` call on the capture
/// client. It represents the instant at which the first sample of the retrieved buffer was
/// captured.
fn input_timestamp(
    stream: &StreamInner,
    buffer_qpc_position: UINT64,
) -> Result<crate::InputStreamTimestamp, StreamError> {
    // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
    let qpc_nanos = buffer_qpc_position as i128 * 100;
    let capture = crate::StreamInstant::from_nanos_i128(qpc_nanos)
        .expect("performance counter out of range of `StreamInstant` representation");
    let callback = stream_instant(stream)?;
    Ok(crate::InputStreamTimestamp { capture, callback })
}

/// Produce the output stream timestamp.
///
/// `frames_available` is the number of frames available for writing as reported by subtracting the
/// result of `GetCurrentPadding` from the maximum buffer size.
///
/// `sample_rate` is the rate at which audio frames are processed by the device.
///
/// TODO: The returned `playback` is an estimate that assumes audio is delivered immediately after
/// `frames_available` are consumed. The reality is that there is likely a tiny amount of latency
/// after this, but not sure how to determine this.
fn output_timestamp(
    stream: &StreamInner,
    frames_available: u32,
    sample_rate: crate::SampleRate,
) -> Result<crate::OutputStreamTimestamp, StreamError> {
    let callback = stream_instant(stream)?;
    let buffer_duration = frames_to_duration(frames_available, sample_rate);
    let playback = callback
        .add(buffer_duration)
        .expect("`playback` occurs beyond representation supported by `StreamInstant`");
    Ok(crate::OutputStreamTimestamp { callback, playback })
}
