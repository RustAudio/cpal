use std::{
    mem,
    ops::ControlFlow,
    ptr, slice,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use windows::Win32::{
    Foundation::{self, PROPERTYKEY, WAIT_OBJECT_0},
    Media::Audio,
    System::{Performance, SystemServices, Threading},
};

use crate::{
    CallbackInfo, Data, Error, ErrorKind, FrameCount, ResultExt, SampleFormat, SampleRate,
    StreamConfig, StreamInstant, StreamTimestamp,
    host::{ErrorCallbackArc, emit_error, equilibrium::fill_equilibrium, latch::Latch},
    traits::StreamTrait,
};

const DEVICE_ERROR_NONE: u8 = 0;
const DEVICE_ERROR_NOT_AVAILABLE: u8 = 1;
const DEVICE_ERROR_CHANGED: u8 = 2;

/// A kernel event whose lifetime is shared by the monitor and the audio run loop.
///
/// The audio loop can outlive `DefaultDeviceMonitor` when a stream is dropped from one of its
/// callbacks. Store the opaque handle as an integer so the shared owner is `Send + Sync`; event
/// handles can be signalled and waited on from any thread.
struct MonitorEvent(usize);

impl MonitorEvent {
    fn new(handle: Foundation::HANDLE) -> Self {
        Self(handle.0 as usize)
    }

    fn handle(&self) -> Foundation::HANDLE {
        Foundation::HANDLE(self.0 as *mut std::ffi::c_void)
    }
}

impl Drop for MonitorEvent {
    fn drop(&mut self) {
        unsafe {
            let _ = Foundation::CloseHandle(self.handle());
        }
    }
}

/// Fires a Windows auto-reset event when the system default audio device changes.
pub(crate) struct DefaultDeviceMonitor {
    stop_event: Foundation::HANDLE,
    worker: Option<JoinHandle<()>>,
    event: Arc<MonitorEvent>,
    pending_device_error: Arc<AtomicU8>,
}

impl DefaultDeviceMonitor {
    pub fn new(flow: Audio::EDataFlow) -> Result<Self, Error> {
        let event = Arc::new(MonitorEvent::new(unsafe {
            Threading::CreateEventW(None, false, false, None).map_err(Error::from)?
        }));
        let notification_event = match unsafe { Threading::CreateEventW(None, false, false, None) }
        {
            Ok(event) => event,
            Err(error) => {
                drop(event);
                return Err(Error::from(error));
            }
        };
        let notification_event = Arc::new(MonitorEvent::new(notification_event));
        let stop_event = match unsafe { Threading::CreateEventW(None, false, false, None) } {
            Ok(event) => event,
            Err(error) => {
                drop(event);
                drop(notification_event);
                return Err(Error::from(error));
            }
        };
        let pending_default_changed = Arc::new(AtomicBool::new(false));
        let pending_device_error = Arc::new(AtomicU8::new(DEVICE_ERROR_NONE));
        let worker_pending_default_changed = pending_default_changed.clone();
        let worker_pending_device_error = pending_device_error.clone();
        // `HANDLE` is an opaque, thread-safe kernel handle, but the windows crate models it as a
        // raw pointer and therefore does not implement `Send`. Move its bit pattern as `usize`.
        let event_handle = event.handle().0 as usize;
        let stop_event_handle = stop_event.0 as usize;
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);

        let worker = match thread::Builder::new()
            .name("cpal_wasapi_device_monitor".to_owned())
            .spawn(move || {
                let event = Foundation::HANDLE(event_handle as *mut std::ffi::c_void);
                let stop_event = Foundation::HANDLE(stop_event_handle as *mut std::ffi::c_void);
                // Keep every IMMDeviceEnumerator call, including unregister, on the apartment
                // that created the enumerator. Notification callbacks only signal an event.
                crate::host::com::com_initialized_mta();
                let enumerator = match super::device::create_enumerator() {
                    Ok(enumerator) => enumerator,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        close_default_device_monitor_handles(stop_event);
                        return;
                    }
                };
                let client: Audio::IMMNotificationClient = DefaultDeviceNotificationImpl {
                    flow,
                    notification_event: Arc::clone(&notification_event),
                    pending_default_changed: worker_pending_default_changed,
                }
                .into();

                if let Err(error) =
                    unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
                {
                    let _ = ready_tx.send(Err(error.to_string()));
                    close_default_device_monitor_handles(stop_event);
                    return;
                }

                if ready_tx.send(Ok(())).is_err() {
                    let unregistered =
                        unsafe { enumerator.UnregisterEndpointNotificationCallback(&client) }
                            .is_ok();
                    close_default_device_monitor_handles_after_unregister(
                        Arc::clone(&notification_event),
                        stop_event,
                        unregistered,
                    );
                    return;
                }

                let handles = [stop_event, notification_event.handle()];
                loop {
                    let result = unsafe {
                        Threading::WaitForMultipleObjectsEx(
                            &handles,
                            false,
                            Threading::INFINITE,
                            false,
                        )
                    };
                    if result == Foundation::WAIT_FAILED {
                        break;
                    }

                    match (result.0 - Foundation::WAIT_OBJECT_0.0) as usize {
                        0 => break,
                        1 => {
                            let default_changed =
                                pending_default_changed.swap(false, Ordering::AcqRel);
                            let default_available =
                                super::device::current_default_endpoint_with(&enumerator, flow)
                                    .is_some();
                            if default_changed || !default_available {
                                let error = if default_available {
                                    DEVICE_ERROR_CHANGED
                                } else {
                                    DEVICE_ERROR_NOT_AVAILABLE
                                };
                                worker_pending_device_error.store(error, Ordering::Release);
                                unsafe {
                                    let _ = Threading::SetEvent(event);
                                }
                            }
                        }
                        _ => break,
                    }
                }

                let unregistered =
                    unsafe { enumerator.UnregisterEndpointNotificationCallback(&client) }.is_ok();
                close_default_device_monitor_handles_after_unregister(
                    Arc::clone(&notification_event),
                    stop_event,
                    unregistered,
                );
            }) {
            Ok(worker) => worker,
            Err(error) => {
                close_default_device_monitor_handles(stop_event);
                return Err(Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to create device notification thread: {error}"),
                ));
            }
        };

        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(Error::with_message(
                    ErrorKind::BackendError,
                    format!("Failed to register device notification callback: {error}"),
                ));
            }
            Err(_) => {
                let _ = worker.join();
                return Err(Error::with_message(
                    ErrorKind::BackendError,
                    "Device notification thread exited before registration completed",
                ));
            }
        }

        Ok(Self {
            stop_event,
            worker: Some(worker),
            event,
            pending_device_error,
        })
    }
}

fn close_default_device_monitor_handles(stop_event: Foundation::HANDLE) {
    unsafe {
        let _ = Foundation::CloseHandle(stop_event);
    }
}

fn close_default_device_monitor_handles_after_unregister(
    notification_event: Arc<MonitorEvent>,
    stop_event: Foundation::HANDLE,
    unregistered: bool,
) {
    if unregistered {
        close_default_device_monitor_handles(stop_event);
    } else {
        // A failed unregister can leave callbacks in flight. Keep the callback event alive in
        // case Windows still invokes the registered client after this worker exits.
        std::mem::forget(notification_event);
        unsafe {
            let _ = Foundation::CloseHandle(stop_event);
        }
    }
}

impl Drop for DefaultDeviceMonitor {
    fn drop(&mut self) {
        unsafe {
            let _ = Threading::SetEvent(self.stop_event);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // `event` remains open until the audio run loop drops its shared reference.
    }
}

#[windows::core::implement(Audio::IMMNotificationClient)]
struct DefaultDeviceNotificationImpl {
    flow: Audio::EDataFlow,
    notification_event: Arc<MonitorEvent>,
    pending_default_changed: Arc<AtomicBool>,
}

impl Audio::IMMNotificationClient_Impl for DefaultDeviceNotificationImpl_Impl {
    fn OnDefaultDeviceChanged(
        &self,
        flow: Audio::EDataFlow,
        role: Audio::ERole,
        _pwstrdefaultdeviceid: &windows::core::PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == self.flow && role == Audio::eConsole {
            self.pending_default_changed.store(true, Ordering::Release);
            unsafe {
                let _ = Threading::SetEvent(self.notification_event.handle());
            }
        }
        Ok(())
    }

    fn OnDeviceStateChanged(
        &self,
        _pwstrdeviceid: &windows::core::PCWSTR,
        dwnewstate: Audio::DEVICE_STATE,
    ) -> windows::core::Result<()> {
        // `DEVICE_STATE_UNPLUGGED`: physical jack disconnected; endpoint still exists in the
        // collection but produces no audio. `OnDeviceRemoved` does *not* fire for this state.
        // `DEVICE_STATE_NOTPRESENT`: hardware absent; endpoint may persist as a ghost record.
        // `DEVICE_STATE_DISABLED`: device was manually disabled by the user.
        //
        let is_unavailable = dwnewstate == Audio::DEVICE_STATE_DISABLED
            || dwnewstate == Audio::DEVICE_STATE_NOTPRESENT
            || dwnewstate == Audio::DEVICE_STATE_UNPLUGGED;
        if is_unavailable {
            // The monitor thread checks for a replacement default without making COM calls from
            // this Windows-managed callback thread.
            unsafe {
                let _ = Threading::SetEvent(self.notification_event.handle());
            }
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, _pwstrdeviceid: &windows::core::PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _pwstrdeviceid: &windows::core::PCWSTR) -> windows::core::Result<()> {
        // The monitor thread checks for a replacement default without making COM calls from this
        // Windows-managed callback thread.
        unsafe {
            let _ = Threading::SetEvent(self.notification_event.handle());
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _pwstrdeviceid: &windows::core::PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

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
    pending_scheduled_event: Foundation::HANDLE,

    // Callback size in frames.
    period_frames: FrameCount,

    // QueryPerformanceFrequency result, cached at construction (constant for the system lifetime).
    qpc_frequency: u64,

    // Present for default-device streams. The monitor and run loop share the event owner, so a
    // self-drop from a callback cannot close a handle the run loop is about to wait on.
    _default_device_monitor: Option<DefaultDeviceMonitor>,

    // Latch that ensures no callbacks fire before the caller receives the `Stream` handle.
    latch: Latch,

    // Raised by `stop()` and `pause()` so the audio loop skips the user callback.
    skip_callback: Arc<AtomicBool>,

    // Latency + buffer fill in microseconds, updated each output callback; zero for input.
    fill_usec: Arc<AtomicU64>,
}

// SAFETY: Stream's event HANDLE is a kernel event that can be signalled from any thread. The
// default-device monitor uses a joined worker, atomic state, and an Arc-owned event handle; the
// run loop retains the event owner until it stops waiting on the handle.
unsafe impl Send for Stream {}

// SAFETY: Shared access to Stream is limited to the thread-safe command sender, atomics, and
// kernel-event signalling. The raw event handle is closed only after the run loop exits or drops
// its shared owner, and the monitor worker is joined during exclusive Stream destruction.
unsafe impl Sync for Stream {}

struct RunContext {
    // Streams that have been created in this event loop.
    stream: StreamInner,

    // Handles corresponding to the `event` field of each element of `voices`. Must always be in
    // sync with `voices`, except that the first element is always `pending_scheduled_event`.
    handles: Vec<Foundation::HANDLE>,

    commands: Receiver<Command>,

    // Set by the monitor thread before it signals a default-device change.
    pending_device_error: Option<Arc<AtomicU8>>,

    // Keeps the monitor event alive until this run loop stops waiting on it.
    _default_device_monitor_event: Option<Arc<MonitorEvent>>,

    // Owned here so the worker thread closes it on exit in a self-join case.
    pending_scheduled_event: Foundation::HANDLE,
}

impl Drop for RunContext {
    fn drop(&mut self) {
        unsafe {
            let _ = Foundation::CloseHandle(self.pending_scheduled_event);
        }
    }
}

// Once we start running the eventloop, the RunContext will not be moved.
unsafe impl Send for RunContext {}

pub enum Command {
    PlayStream,
    PauseStream,
    StopStream,
    Terminate,
}

pub enum AudioClientFlow {
    Render {
        render_client: Audio::IAudioRenderClient,
    },
    Capture {
        capture_client: Audio::IAudioCaptureClient,
    },
}

/// Play/pause state of a [`StreamInner`]. `Priming` only ever applies to Render streams: set by
/// a cold-start `PlayStream`, it defers `Start()` until the run loop lands a real fill in the
/// buffer, so playback begins with actual audio instead of a silence-padded first period.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Priming,
    Playing,
}

pub struct StreamInner {
    pub audio_client: Audio::IAudioClient,
    pub audio_clock: Audio::IAudioClock,
    pub client_flow: AudioClientFlow,
    // Event that is signalled by WASAPI whenever audio data must be written.
    pub event: Foundation::HANDLE,
    // Current playback state of the stream.
    pub playback_state: PlaybackState,
    // Number of frames of audio data in the underlying buffer allocated by WASAPI.
    pub max_frames_in_buffer: FrameCount,
    // Callback size in frames.
    pub period_frames: FrameCount,
    // Number of bytes that each frame occupies.
    pub bytes_per_frame: u16,
    // The configuration with which the stream was created.
    pub config: StreamConfig,
    // The sample format with which the stream was created.
    pub sample_format: SampleFormat,
    // Hardware pipeline latency.
    pub stream_latency: Duration,
    // Raised by `stop()` and `pause()` so the audio loop skips the user callback.
    pub skip_callback: Arc<AtomicBool>,
    // Updated each output callback: latency + current buffer fill in microseconds.
    pub fill_usec: Arc<AtomicU64>,
}

impl Stream {
    pub(crate) fn new_input<D>(
        stream_inner: StreamInner,
        mut data_callback: D,
        error_callback: ErrorCallbackArc,
        default_device_monitor: Option<DefaultDeviceMonitor>,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
    {
        let pending_scheduled_event = unsafe {
            Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
        }
        .expect("cpal: could not create input stream event");
        let (tx, rx) = channel();

        let period_frames = stream_inner.period_frames;
        let skip_callback = stream_inner.skip_callback.clone();
        let fill_usec = stream_inner.fill_usec.clone();
        let mut qpc_frequency: i64 = 0;
        unsafe {
            Performance::QueryPerformanceFrequency(&mut qpc_frequency)
                .expect("QueryPerformanceFrequency failed");
            debug_assert_ne!(qpc_frequency, 0, "QueryPerformanceFrequency returned zero");
        }

        let mut handles = vec![pending_scheduled_event, stream_inner.event];
        let default_device_monitor_event = default_device_monitor
            .as_ref()
            .map(|monitor| Arc::clone(&monitor.event));
        if let Some(ref event) = default_device_monitor_event {
            handles.push(event.handle());
        }

        let pending_device_error = default_device_monitor
            .as_ref()
            .map(|m| m.pending_device_error.clone());
        let run_context = RunContext {
            handles,
            stream: stream_inner,
            commands: rx,
            pending_device_error,
            _default_device_monitor_event: default_device_monitor_event,
            pending_scheduled_event,
        };

        // The latch is released just before the `Stream` is returned so the worker cannot fire any
        // callbacks before the caller has the handle.
        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let thread = thread::Builder::new()
            .name("cpal_wasapi_in".to_owned())
            .spawn(move || {
                waiter.wait();
                run_input(run_context, &mut data_callback, &error_callback)
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to create audio thread: {e}"),
                )
            })?;

        latch.add_thread(thread.thread().clone());
        let stream = Stream {
            thread: Some(thread),
            commands: tx,
            pending_scheduled_event,
            period_frames,
            qpc_frequency: qpc_frequency as u64,
            _default_device_monitor: default_device_monitor,
            latch,
            skip_callback,
            fill_usec,
        };
        Ok(stream)
    }

    pub(crate) fn new_output<D>(
        stream_inner: StreamInner,
        mut data_callback: D,
        error_callback: ErrorCallbackArc,
        default_device_monitor: Option<DefaultDeviceMonitor>,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
    {
        let pending_scheduled_event = unsafe {
            Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
        }
        .expect("cpal: could not create output stream event");
        let (tx, rx) = channel();

        let period_frames = stream_inner.period_frames;
        let skip_callback = stream_inner.skip_callback.clone();
        let fill_usec = stream_inner.fill_usec.clone();
        let mut qpc_frequency: i64 = 0;
        unsafe {
            Performance::QueryPerformanceFrequency(&mut qpc_frequency)
                .expect("QueryPerformanceFrequency failed");
            debug_assert_ne!(qpc_frequency, 0, "QueryPerformanceFrequency returned zero");
        }

        let mut handles = vec![pending_scheduled_event, stream_inner.event];
        let default_device_monitor_event = default_device_monitor
            .as_ref()
            .map(|monitor| Arc::clone(&monitor.event));
        if let Some(ref event) = default_device_monitor_event {
            handles.push(event.handle());
        }

        let pending_device_error = default_device_monitor
            .as_ref()
            .map(|m| m.pending_device_error.clone());
        let run_context = RunContext {
            handles,
            stream: stream_inner,
            commands: rx,
            pending_device_error,
            _default_device_monitor_event: default_device_monitor_event,
            pending_scheduled_event,
        };

        // The latch is released just before the `Stream` is returned so the worker cannot fire any
        // callbacks before the caller has the handle.
        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let thread = thread::Builder::new()
            .name("cpal_wasapi_out".to_owned())
            .spawn(move || {
                waiter.wait();
                run_output(run_context, &mut data_callback, &error_callback)
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to create audio thread: {e}"),
                )
            })?;

        latch.add_thread(thread.thread().clone());
        let stream = Stream {
            thread: Some(thread),
            commands: tx,
            pending_scheduled_event,
            period_frames,
            qpc_frequency: qpc_frequency as u64,
            _default_device_monitor: default_device_monitor,
            latch,
            skip_callback,
            fill_usec,
        };
        Ok(stream)
    }

    /// Releases the latch so the worker thread can begin processing audio callbacks.
    pub(crate) fn signal_ready(&self) {
        self.latch.release();
    }

    fn push_command(&self, command: Command) -> Result<(), Error> {
        let invalidated = || {
            Error::with_message(
                ErrorKind::StreamInvalidated,
                "Stream command channel closed",
            )
        };
        self.commands.send(command).map_err(|_| invalidated())?;
        unsafe {
            // The worker thread may have already closed this handle on exit.
            Threading::SetEvent(self.pending_scheduled_event).map_err(|_| invalidated())?;
        }
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Release the latch in case the stream is dropped before signal_ready() was called.
        self.signal_ready();

        let _ = self.push_command(Command::Terminate);
        if let Some(handle) = self.thread.take() {
            // Prevent self-join: Terminate was sent; the thread exits after the current callback
            // returns. pending_scheduled_event is closed by RunContext::drop on the worker thread,
            // covering both the self-join case (where we cannot join here) and the normal case
            // (where the thread exits and drops RunContext before join() returns).
            if handle.thread().id() != std::thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        // Clear any pending drain so the audio loop resumes calling the user callback.
        self.skip_callback.store(false, Ordering::Relaxed);
        self.push_command(Command::PlayStream).map_err(|_| {
            Error::with_message(
                ErrorKind::StreamInvalidated,
                "Stream command channel closed",
            )
        })?;
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        self.skip_callback.store(true, Ordering::Relaxed);
        self.push_command(Command::PauseStream)
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        self.skip_callback.store(true, Ordering::Relaxed);

        if timeout != Some(Duration::ZERO) {
            let fill = Duration::from_micros(self.fill_usec.load(Ordering::Relaxed));
            let wait = timeout.map_or(fill, |t| fill.min(t));
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }

        self.push_command(Command::StopStream).map_err(|_| {
            Error::with_message(
                ErrorKind::StreamInvalidated,
                "Stream command channel closed",
            )
        })?;
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        let mut counter: i64 = 0;
        unsafe {
            Performance::QueryPerformanceCounter(&mut counter)
                .expect("QueryPerformanceCounter failed");
        }
        // Convert to 100-nanosecond units first, matching the precision of WASAPI QPCPosition
        // values delivered to callbacks. This keeps `now()` on the same 100 ns grid as
        // callback/capture/playback instants, avoiding false sub-100 ns deltas.
        let units_100ns = counter as u128 * 10_000_000 / self.qpc_frequency as u128;
        let nanos = units_100ns * 100;
        StreamInstant::new(
            (nanos / 1_000_000_000) as u64,
            (nanos % 1_000_000_000) as u32,
        )
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.period_frames)
    }
}

impl Drop for StreamInner {
    fn drop(&mut self) {
        unsafe {
            let _ = Foundation::CloseHandle(self.event);
        }
    }
}

// Process any pending commands that are queued within the `RunContext`.
// Returns `true` if the loop should continue running, `false` if it should terminate.
fn process_commands(run_context: &mut RunContext) -> Result<bool, Error> {
    // Process the pending commands.
    for command in run_context.commands.try_iter() {
        match command {
            Command::PlayStream => unsafe {
                if run_context.stream.playback_state == PlaybackState::Stopped {
                    // PlayStream also fires on resume from pause, where the buffer wasn't reset
                    // and may already hold real, unplayed data.
                    let cold_start = match run_context.stream.client_flow {
                        AudioClientFlow::Render { .. } => {
                            get_available_frames(&run_context.stream)? > 0
                        }
                        AudioClientFlow::Capture { .. } => false,
                    };
                    if cold_start {
                        // Defer Start() until the run loop lands a real fill in the buffer, so
                        // playback begins with actual audio.
                        run_context.stream.playback_state = PlaybackState::Priming;
                    } else {
                        run_context
                            .stream
                            .audio_client
                            .Start()
                            .context("Failed to start audio client")?;
                        run_context.stream.playback_state = PlaybackState::Playing;
                    }
                }
            },
            Command::PauseStream | Command::StopStream => unsafe {
                if run_context.stream.playback_state == PlaybackState::Playing {
                    run_context
                        .stream
                        .audio_client
                        .Stop()
                        .context("Failed to stop audio client")?;
                }
                // Also cancels a deferred prefill that hasn't run yet, e.g. pause called right
                // after play, before the run loop got a chance to act on it.
                run_context.stream.playback_state = PlaybackState::Stopped;
                if matches!(command, Command::StopStream) {
                    // Reset discards the render buffer so a future Start() plays no stale frames.
                    run_context
                        .stream
                        .audio_client
                        .Reset()
                        .context("Failed to reset audio client")?;
                }
            },
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
fn wait_for_handle_signal(handles: &[Foundation::HANDLE]) -> Result<usize, Error> {
    debug_assert!(handles.len() <= SystemServices::MAXIMUM_WAIT_OBJECTS as usize);
    let result = unsafe {
        Threading::WaitForMultipleObjectsEx(
            handles,
            false,               // Don't wait for all, just wait for the first
            Threading::INFINITE, // TODO: allow setting a timeout
            false,               // irrelevant parameter here
        )
    };
    if result == Foundation::WAIT_FAILED {
        return Err(Error::with_message(
            ErrorKind::StreamInvalidated,
            "Failed to wait for audio event",
        ));
    }
    // Notifying the corresponding task handler.
    let handle_idx = (result.0 - WAIT_OBJECT_0.0) as usize;
    Ok(handle_idx)
}

// Get the number of available frames that are available for writing/reading.
#[inline]
fn get_available_frames(stream: &StreamInner) -> Result<FrameCount, Error> {
    unsafe {
        let padding = stream
            .audio_client
            .GetCurrentPadding()
            .context("Failed to get current padding")?;
        Ok(stream.max_frames_in_buffer - padding)
    }
}

fn run_input(
    mut run_ctxt: RunContext,
    data_callback: &mut dyn FnMut(&Data, &CallbackInfo),
    error_callback: &ErrorCallbackArc,
) {
    #[cfg(feature = "realtime")]
    let _rt_priority = boost_current_thread_priority(
        run_ctxt.stream.period_frames,
        run_ctxt.stream.config.sample_rate,
    )
    .map_err(|err| emit_error(error_callback, err))
    .unwrap_or_default();

    let stream = &run_ctxt.stream;
    let scratch_len = if stream.sample_format == SampleFormat::I24 {
        stream.max_frames_in_buffer as usize * stream.bytes_per_frame as usize / size_of::<i32>()
    } else {
        // The scratch buffer won't be used in this case.
        0 // Vec::with_capacity(0) does not allocate.
    };
    let mut scratch_buffer = vec![0; scratch_len].into_boxed_slice();

    loop {
        match process_commands_and_await_signal(&mut run_ctxt, error_callback) {
            ControlFlow::Break(()) => break,
            ControlFlow::Continue(false) => continue,
            ControlFlow::Continue(true) => {}
        }
        let capture_client = match run_ctxt.stream.client_flow {
            AudioClientFlow::Capture { ref capture_client } => capture_client.clone(),
            _ => unreachable!(),
        };
        if let Err(err) = process_input(
            &run_ctxt.stream,
            capture_client,
            data_callback,
            &mut scratch_buffer,
        ) {
            emit_error(error_callback, err);
            break;
        }
    }
}

fn run_output(
    mut run_ctxt: RunContext,
    data_callback: &mut dyn FnMut(&mut Data, &CallbackInfo),
    error_callback: &ErrorCallbackArc,
) {
    #[cfg(feature = "realtime")]
    let _rt_priority = boost_current_thread_priority(
        run_ctxt.stream.period_frames,
        run_ctxt.stream.config.sample_rate,
    )
    .map_err(|err| emit_error(error_callback, err))
    .unwrap_or_default();

    // The clock frequency is constant for the stream's lifetime.
    let clock_frequency = match unsafe { run_ctxt.stream.audio_clock.GetFrequency() }
        .context("Failed to get audio clock frequency")
    {
        Ok(0) => {
            emit_error(
                error_callback,
                Error::with_message(
                    ErrorKind::BackendError,
                    "IAudioClock::GetFrequency returned zero",
                ),
            );
            return;
        }
        Ok(frequency) => frequency,
        Err(err) => {
            emit_error(error_callback, err);
            return;
        }
    };
    let mut frames_written: u64 = 0;

    loop {
        match process_commands_and_await_signal(&mut run_ctxt, error_callback) {
            ControlFlow::Break(()) => break,
            ControlFlow::Continue(false) => continue,
            ControlFlow::Continue(true) => {}
        }
        let render_client = match run_ctxt.stream.client_flow {
            AudioClientFlow::Render { ref render_client } => render_client.clone(),
            _ => unreachable!(),
        };
        if let Err(err) = process_output(
            &run_ctxt.stream,
            render_client,
            data_callback,
            clock_frequency,
            &mut frames_written,
        ) {
            emit_error(error_callback, err);
            break;
        }
        if run_ctxt.stream.playback_state == PlaybackState::Priming {
            if run_ctxt.stream.skip_callback.load(Ordering::Relaxed) {
                // process_output submitted nothing this cycle; stay stopped.
                run_ctxt.stream.playback_state = PlaybackState::Stopped;
            } else {
                // The buffer above just received a real fill; start now so playback begins with it.
                let start_result = unsafe { run_ctxt.stream.audio_client.Start() }
                    .context("Failed to start audio client");
                if let Err(err) = start_result {
                    emit_error(error_callback, err);
                    break;
                }
                run_ctxt.stream.playback_state = PlaybackState::Playing;
            }
        }
    }
}

/// Holds the MMCSS registration made by `audio_thread_priority` and reverts it on drop.
#[cfg(feature = "realtime")]
#[derive(Default)]
struct RtPriority(Option<audio_thread_priority::RtPriorityHandle>);

#[cfg(feature = "realtime")]
impl Drop for RtPriority {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // Nothing useful to do if reverting fails while the thread is exiting.
            let _ = audio_thread_priority::demote_current_thread_from_real_time(handle);
        }
    }
}

/// Attempts to elevate the current thread to real-time or high-priority scheduling.
#[cfg(feature = "realtime")]
fn boost_current_thread_priority(
    period_frames: FrameCount,
    sample_rate: SampleRate,
) -> Result<RtPriority, Error> {
    match audio_thread_priority::promote_current_thread_to_real_time(period_frames, sample_rate) {
        Ok(handle) => Ok(RtPriority(Some(handle))),
        Err(_) => unsafe {
            let thread_handle = Threading::GetCurrentThread();
            Threading::SetThreadPriority(thread_handle, Threading::THREAD_PRIORITY_TIME_CRITICAL)
                .map(|()| RtPriority::default())
                .context("Failed to promote audio thread to real-time priority")
        },
    }
}

fn default_device_change_error(reason: u8) -> Option<Error> {
    match reason {
        DEVICE_ERROR_NOT_AVAILABLE => Some(ErrorKind::DeviceNotAvailable.into()),
        DEVICE_ERROR_CHANGED => Some(ErrorKind::DeviceChanged.into()),
        _ => None,
    }
}

fn take_default_device_change_error(run_context: &RunContext) -> Option<Error> {
    let reason = run_context
        .pending_device_error
        .as_ref()?
        .swap(DEVICE_ERROR_NONE, Ordering::AcqRel);
    default_device_change_error(reason)
}

fn process_commands_and_await_signal(
    run_context: &mut RunContext,
    error_callback: &ErrorCallbackArc,
) -> ControlFlow<(), bool> {
    // Process queued commands.
    match process_commands(run_context) {
        Ok(true) => (),
        Ok(false) => return ControlFlow::Break(()),
        Err(err) => {
            emit_error(error_callback, err);
            return ControlFlow::Break(());
        }
    };

    if run_context.stream.playback_state == PlaybackState::Priming {
        // The stream hasn't started yet, so its event never fires. Write the priming fill now
        // instead of waiting on it.
        return ControlFlow::Continue(true);
    }

    if let Some(error) = take_default_device_change_error(run_context) {
        emit_error(error_callback, error);
    }

    // Wait for any of the handles to be signalled.
    let handle_idx = match wait_for_handle_signal(&run_context.handles) {
        Ok(idx) => idx,
        Err(err) => {
            emit_error(error_callback, err);
            return ControlFlow::Break(());
        }
    };

    // Handle layout: 0 = pending_scheduled_event (commands), 1 = WASAPI audio event,
    // 2+ = default-device change event (only present for default-device streams).
    // Continue(true)  = audio event fired, proceed to process audio this iteration.
    // Continue(false) = command or device-change event, loop around and wait again.
    if handle_idx >= 2 {
        if let Some(error) = take_default_device_change_error(run_context) {
            emit_error(error_callback, error);
        }
        return ControlFlow::Continue(false);
    }
    ControlFlow::Continue(handle_idx != 0)
}

// The loop for processing pending input data.
fn process_input(
    stream: &StreamInner,
    capture_client: Audio::IAudioCaptureClient,
    data_callback: &mut dyn FnMut(&Data, &CallbackInfo),
    scratch_buffer: &mut [i32],
) -> Result<(), Error> {
    unsafe {
        // Get the available data in the shared buffer.
        let mut buffer: *mut u8 = ptr::null_mut();
        let mut flags = mem::MaybeUninit::uninit();
        loop {
            let mut frames_available = match capture_client.GetNextPacketSize() {
                Ok(0) => return Ok(()),
                Ok(f) => f,
                Err(err) => return Err(Error::from(err)),
            };
            let mut qpc_position: u64 = 0;
            let mut device_position: u64 = 0;
            let result = capture_client.GetBuffer(
                &mut buffer,
                &mut frames_available,
                flags.as_mut_ptr(),
                Some(&mut device_position),
                Some(&mut qpc_position),
            );

            match result {
                // TODO: Can this happen?
                Err(e) if e.code() == Audio::AUDCLNT_S_BUFFER_EMPTY => continue,
                Err(e) => return Err(Error::from(e)),
                Ok(_) => (),
            }

            let flags = flags.assume_init();
            // The discontinuity flag is undefined on the first GetBuffer after Start,
            // where device_position is still 0.
            let xrun = device_position != 0
                && flags & Audio::AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0;

            debug_assert!(!buffer.is_null());
            let byte_count = frames_available as usize * stream.bytes_per_frame as usize;
            let data = if stream.sample_format == SampleFormat::I24 {
                // WASAPI stores i24 in the upper bits
                let source_data =
                    slice::from_raw_parts(buffer.cast(), byte_count / size_of::<i32>());
                // use a scratch buffer since the capture buffer isn't meant to be written
                let dst = &mut scratch_buffer[..source_data.len()];
                dst.copy_from_slice(source_data);
                for sample in dst.iter_mut() {
                    // On signed integers, >> is an arithmetic shift,
                    // which ensures the correct upper bits get shifted in
                    *sample >>= 8;
                }

                dst.as_mut_ptr().cast()
            } else {
                buffer.cast()
            };

            let len = byte_count / stream.sample_format.sample_size();
            let data = Data::from_parts(data, len, stream.sample_format);

            if !stream.skip_callback.load(Ordering::Relaxed) {
                // The `qpc_position` is in 100 nanosecond units. Convert it to nanoseconds.
                let timestamp = input_timestamp(stream, qpc_position)?;
                data_callback(&data, &CallbackInfo { timestamp, xrun });
            }

            // Release the buffer.
            capture_client
                .ReleaseBuffer(frames_available)
                .context("Failed to release capture buffer")?;
        }
    }
}

// The loop for writing output data.
fn process_output(
    stream: &StreamInner,
    render_client: Audio::IAudioRenderClient,
    data_callback: &mut dyn FnMut(&mut Data, &CallbackInfo),
    clock_frequency: u64,
    frames_written: &mut u64,
) -> Result<(), Error> {
    // The number of frames available for writing.
    let frames_available = match get_available_frames(stream)? {
        0 => return Ok(()), // TODO: Can this happen?
        n => n,
    };

    let padding = stream.max_frames_in_buffer - frames_available;
    let fill_usec = (padding as u64)
        .saturating_mul(1_000_000)
        .saturating_div(stream.config.sample_rate as u64)
        .saturating_add(
            stream
                .stream_latency
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
        );
    stream.fill_usec.store(fill_usec, Ordering::Relaxed);

    if stream.skip_callback.load(Ordering::Relaxed) {
        // Skip the period instead of queuing silence a future resume would replay.
        return Ok(());
    }

    unsafe {
        let buffer = render_client.GetBuffer(frames_available)?;

        debug_assert!(!buffer.is_null());

        let byte_count = frames_available as usize * stream.bytes_per_frame as usize;
        let buffer_slice = std::slice::from_raw_parts_mut(buffer, byte_count);
        fill_equilibrium(buffer_slice, stream.sample_format);

        let data = buffer as *mut ();
        let len = byte_count / stream.sample_format.sample_size();
        let mut data = Data::from_parts(data, len, stream.sample_format);
        let sample_rate = stream.config.sample_rate;
        let timestamp = output_timestamp(stream, sample_rate, clock_frequency, *frames_written)?;
        // WASAPI exposes no render-side xrun signal.
        data_callback(
            &mut data,
            &CallbackInfo {
                timestamp,
                xrun: false,
            },
        );

        if stream.sample_format == SampleFormat::I24 {
            // WASAPI stores i24 in the upper bits
            #[expect(
                clippy::cast_ptr_alignment,
                reason = "WASAPI guarantees the buffer to be aligned to a frame boundary"
            )]
            let buffer_slice_i32 = slice::from_raw_parts_mut(buffer.cast::<i32>(), len);
            for sample in buffer_slice_i32 {
                *sample <<= 8;
            }
        }

        render_client.ReleaseBuffer(frames_available, 0)?;

        *frames_written += frames_available as u64;
    }

    Ok(())
}

/// Reads the stream's `IAudioClock` in a single `GetPosition` call, returning the callback
/// [`StreamInstant`] together with the device position from that same snapshot.
#[inline]
fn clock_position(stream: &StreamInner) -> Result<(StreamInstant, u64), Error> {
    let mut position: u64 = 0;
    let mut qpc_position: u64 = 0;
    unsafe {
        stream
            .audio_clock
            .GetPosition(&mut position, Some(&mut qpc_position))
            .context("Failed to get clock position")?;
    };
    // The `qpc_position` is in 100-nanosecond units.
    let nanos = qpc_position as u128 * 100;
    let instant = StreamInstant::new(
        (nanos / 1_000_000_000) as u64,
        (nanos % 1_000_000_000) as u32,
    );
    Ok((instant, position))
}

/// Produce the input stream timestamp.
///
/// `buffer_qpc_position` is the `qpc_position` returned via the `GetBuffer` call on the capture
/// client. It represents the instant at which the first sample of the retrieved buffer was
/// captured.
#[inline]
fn input_timestamp(
    stream: &StreamInner,
    buffer_qpc_position: u64,
) -> Result<StreamTimestamp, Error> {
    // The `qpc_position` is in 100-nanosecond units.
    let nanos = buffer_qpc_position as u128 * 100;
    let capture = StreamInstant::new(
        (nanos / 1_000_000_000) as u64,
        (nanos % 1_000_000_000) as u32,
    );
    let (callback, _position) = clock_position(stream)?;
    Ok(StreamTimestamp {
        device: capture,
        callback,
    })
}

/// Produce the output stream timestamp.
///
/// `sample_rate` is the rate at which audio frames are processed by the device.
///
/// `clock_frequency` is the device clock's constant tick rate, used to convert the reported clock
/// position into a played-out duration.
///
/// `frames_written` is the running total of frames submitted to the render buffer so far, used to
/// derive how much audio is buffered ahead of the device position.
#[inline]
fn output_timestamp(
    stream: &StreamInner,
    sample_rate: SampleRate,
    clock_frequency: u64,
    frames_written: u64,
) -> Result<StreamTimestamp, Error> {
    let (callback, position) = clock_position(stream)?;
    // `buffered` is the amount of audio we've already submitted that has not yet been consumed by
    // the device at this instant; it determines when the next written frame will be heard.
    let consumed_nanos = position as u128 * 1_000_000_000 / clock_frequency as u128;
    let written_nanos = frames_written as u128 * 1_000_000_000 / sample_rate as u128;
    let buffered_nanos =
        u64::try_from(written_nanos.saturating_sub(consumed_nanos)).unwrap_or(u64::MAX);
    let buffered = Duration::from_nanos(buffered_nanos);

    let playback = callback + (buffered + stream.stream_latency);
    Ok(StreamTimestamp {
        callback,
        device: playback,
    })
}

#[cfg(test)]
mod tests {
    use super::{Audio, DefaultDeviceMonitor, DefaultDeviceNotificationImpl, MonitorEvent};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use windows::Win32::{
        Foundation::{CO_E_NOTINITIALIZED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::{
            Com::{
                APTTYPE, APTTYPE_MAINSTA, APTTYPE_MTA, APTTYPE_STA, APTTYPEQUALIFIER,
                CoGetApartmentType,
            },
            Threading,
        },
    };
    use windows::core::PCWSTR;

    #[test]
    fn notification_callbacks_do_not_initialize_com() {
        let notification_event = Arc::new(MonitorEvent::new(unsafe {
            Threading::CreateEventW(None, false, false, None)
                .expect("notification event should be created")
        }));

        std::thread::spawn(move || {
            let notification_event_handle = notification_event.handle();
            let apartment_state = || {
                let mut apartment_type = APTTYPE(-1);
                let mut apartment_qualifier = APTTYPEQUALIFIER(0);
                match unsafe { CoGetApartmentType(&mut apartment_type, &mut apartment_qualifier) } {
                    Ok(()) => Some((apartment_type, apartment_qualifier)),
                    Err(error) if error.code() == CO_E_NOTINITIALIZED => None,
                    Err(error) => {
                        panic!("failed to query the test thread's COM apartment: {error}")
                    }
                }
            };
            let pending_default_changed = Arc::new(AtomicBool::new(false));
            let notification: Audio::IMMNotificationClient = DefaultDeviceNotificationImpl {
                flow: Audio::eRender,
                notification_event: Arc::clone(&notification_event),
                pending_default_changed: pending_default_changed.clone(),
            }
            .into();
            let before = apartment_state();
            drop(notification_event);
            let device_id = PCWSTR::null();

            unsafe {
                notification
                    .OnDefaultDeviceChanged(Audio::eRender, Audio::eConsole, device_id)
                    .expect("default-device callback should succeed");
            }
            assert!(pending_default_changed.load(Ordering::Acquire));
            assert_eq!(
                unsafe { Threading::WaitForSingleObject(notification_event_handle, 0) },
                WAIT_OBJECT_0
            );

            unsafe {
                notification
                    .OnDeviceStateChanged(device_id, Audio::DEVICE_STATE_UNPLUGGED)
                    .expect("device-state callback should succeed");
            }
            assert_eq!(
                unsafe { Threading::WaitForSingleObject(notification_event_handle, 0) },
                WAIT_OBJECT_0
            );

            unsafe {
                notification
                    .OnDeviceRemoved(device_id)
                    .expect("device-removed callback should succeed");
            }
            assert_eq!(
                unsafe { Threading::WaitForSingleObject(notification_event_handle, 0) },
                WAIT_OBJECT_0
            );

            assert_eq!(
                apartment_state(),
                before,
                "notification callbacks must not initialize their caller's COM apartment"
            );

            drop(notification);
            assert_eq!(
                unsafe { Threading::WaitForSingleObject(notification_event_handle, 0) },
                WAIT_FAILED,
                "the callback event should close when its final owner is dropped"
            );
        })
        .join()
        .expect("worker thread should complete");
    }

    #[test]
    fn default_endpoint_queries_initialize_uninitialized_threads_as_mta() {
        std::thread::spawn(|| {
            let mut apartment_type = APTTYPE(-1);
            let mut apartment_qualifier = APTTYPEQUALIFIER(0);
            let before =
                unsafe { CoGetApartmentType(&mut apartment_type, &mut apartment_qualifier) };
            assert_eq!(
                before.unwrap_err().code(),
                CO_E_NOTINITIALIZED,
                "test worker should start without COM initialized"
            );

            let _ = super::super::device::current_default_endpoint(Audio::eRender);

            unsafe { CoGetApartmentType(&mut apartment_type, &mut apartment_qualifier) }
                .expect("default endpoint query should initialize COM");
            assert_eq!(
                apartment_type, APTTYPE_MTA,
                "default endpoint queries must not initialize pool workers as STAs"
            );
        })
        .join()
        .expect("worker thread should complete");
    }

    #[test]
    fn default_endpoint_query_preserves_an_existing_sta() {
        std::thread::spawn(|| {
            crate::host::com::com_initialized();
            let mut apartment_type = APTTYPE(-1);
            let mut apartment_qualifier = APTTYPEQUALIFIER(0);
            unsafe { CoGetApartmentType(&mut apartment_type, &mut apartment_qualifier) }
                .expect("test thread should be initialized as an STA");
            assert!(
                apartment_type == APTTYPE_STA || apartment_type == APTTYPE_MAINSTA,
                "test thread should be initialized as an STA"
            );

            let _ = super::super::device::current_default_endpoint(Audio::eRender);

            unsafe { CoGetApartmentType(&mut apartment_type, &mut apartment_qualifier) }
                .expect("default endpoint query should leave COM initialized");
            assert!(
                apartment_type == APTTYPE_STA || apartment_type == APTTYPE_MAINSTA,
                "default endpoint queries must preserve a caller's existing STA"
            );
        })
        .join()
        .expect("worker thread should complete");
    }

    #[test]
    fn monitor_event_stays_open_until_the_run_loop_releases_it() {
        let handle = unsafe {
            Threading::CreateEventW(None, false, false, None)
                .expect("monitor event should be created")
        };
        let event = Arc::new(MonitorEvent::new(handle));
        let run_loop_event = Arc::clone(&event);
        let raw_handle = event.handle();

        drop(event);
        assert_eq!(
            unsafe { Threading::WaitForSingleObject(raw_handle, 0) },
            WAIT_TIMEOUT,
            "dropping the monitor must not close an event still owned by the run loop"
        );

        drop(run_loop_event);
    }

    #[test]
    fn default_device_monitor_registers_on_its_mta_worker() {
        let monitor = DefaultDeviceMonitor::new(Audio::eRender)
            .expect("device monitor should register on its MTA worker");
        drop(monitor);
    }
}
