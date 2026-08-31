use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use super::{
    AlsaContext, POLL_INFINITE, alsa,
    alsa::poll::Descriptors,
    duplex::{duplex_stream_worker, start_duplex},
    timestamp::{callback_instant_for, status_with_timestamp},
    trigger::{TriggerReceiver, TriggerSender, trigger},
    worker::{input_stream_worker, output_stream_worker},
};
use crate::{
    CallbackInfo, Data, DeviceDirection, DuplexCallbackInfo, Error, FrameCount, SampleFormat,
    SampleRate, StreamInstant,
    host::{
        Notify,
        equilibrium::{DSD_EQUILIBRIUM_BYTE, U8_EQUILIBRIUM_BYTE, fill_equilibrium},
        latch::Latch,
    },
    traits::StreamTrait,
};

#[derive(Debug)]
pub struct Stream {
    /// The high-priority audio processing thread calling callbacks.
    /// Option used for moving out in destructor.
    thread: Option<JoinHandle<()>>,

    /// Single-direction or duplex.
    kind: StreamKind,

    /// Used to signal to stop processing.
    trigger: TriggerSender,

    /// Keeps the read end of the self-pipe alive for the lifetime of the Stream, so that
    /// `trigger.wakeup()` never writes to a closed pipe, even if the worker exited early.
    _rx: Arc<TriggerReceiver>,

    /// Latch that blocks the worker thread until `play()` is called for the first time.
    latch: Latch,
}

#[derive(Debug)]
enum StreamKind {
    Single(Arc<StreamInner>),
    Duplex(Arc<DuplexStreamInner>),
}

impl Stream {
    /// Parks the worker and gets exclusive access to the PCM handle(s).
    fn park_worker(&self) {
        self.latch.release();
        // Must be true before the trigger fires, so the worker sees it on the next loop iteration.
        match &self.kind {
            StreamKind::Single(inner) => {
                inner.control.parked.store(true, Ordering::Relaxed);
                self.trigger.wakeup();
                inner.control.park_worker();
            }
            StreamKind::Duplex(inner) => {
                inner.control.parked.store(true, Ordering::Relaxed);
                self.trigger.wakeup();
                inner.control.park_worker();
            }
        }
    }

    pub(super) fn new_input<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
        timeout: Option<Duration>,
    ) -> Stream
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (tx, rx) = trigger();
        let rx_thread = rx.clone();
        let stream = inner.clone();

        // The latch is released by play(); the worker blocks here until then, keeping the PCM
        // in PREPARED state with no DMA activity.
        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let thread = thread::Builder::new()
            .name("cpal_alsa_in".to_owned())
            .spawn(move || {
                waiter.wait();
                input_stream_worker(
                    rx_thread,
                    &stream,
                    &mut data_callback,
                    &mut error_callback,
                    timeout,
                );
            })
            .unwrap();
        latch.add_thread(thread.thread().clone());

        Self {
            thread: Some(thread),
            kind: StreamKind::Single(inner),
            trigger: tx,
            _rx: rx,
            latch,
        }
    }

    pub(super) fn new_output<D, E>(
        inner: Arc<StreamInner>,
        mut data_callback: D,
        mut error_callback: E,
        timeout: Option<Duration>,
    ) -> Stream
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (tx, rx) = trigger();
        let rx_thread = rx.clone();
        let stream = inner.clone();

        // The latch is released by play(); the worker blocks here until then, keeping the PCM
        // in PREPARED state with no DMA activity.
        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let thread = thread::Builder::new()
            .name("cpal_alsa_out".to_owned())
            .spawn(move || {
                waiter.wait();
                output_stream_worker(
                    rx_thread,
                    &stream,
                    &mut data_callback,
                    &mut error_callback,
                    timeout,
                );
            })
            .unwrap();
        latch.add_thread(thread.thread().clone());

        Self {
            thread: Some(thread),
            kind: StreamKind::Single(inner),
            trigger: tx,
            _rx: rx,
            latch,
        }
    }

    pub(super) fn new_duplex<D, E>(
        inner: Arc<DuplexStreamInner>,
        mut data_callback: D,
        mut error_callback: E,
        timeout: Option<Duration>,
    ) -> Stream
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (tx, rx) = trigger();
        let rx_thread = rx.clone();
        let stream = inner.clone();

        // The latch is released by play(); the worker blocks here until then, keeping both
        // PCMs in PREPARED state with no DMA activity.
        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let thread = thread::Builder::new()
            .name("cpal_alsa_duplex".to_owned())
            .spawn(move || {
                waiter.wait();
                duplex_stream_worker(
                    rx_thread,
                    &stream,
                    &mut data_callback,
                    &mut error_callback,
                    timeout,
                );
            })
            .unwrap();
        latch.add_thread(thread.thread().clone());

        Self {
            thread: Some(thread),
            kind: StreamKind::Duplex(inner),
            trigger: tx,
            _rx: rx,
            latch,
        }
    }

    fn suspend_pcm(&self, inner: &StreamInner) -> Result<(), Error> {
        let hw_params = inner.handle.hw_params_current()?;
        if hw_params.can_pause() {
            if inner.handle.state() != alsa::pcm::State::Paused {
                inner.handle.pause(true)?;
            }
        } else {
            self.park_worker();
            let result = if inner.handle.state() == alsa::pcm::State::Running {
                inner
                    .handle
                    .drop()
                    .and_then(|_| inner.handle.prepare())
                    .map_err(Error::from)
            } else {
                Ok(())
            };
            inner.control.unpark_worker();
            return result;
        }
        Ok(())
    }

    // Drops buffered PCM data so a resumed stream doesn't deliver stale audio.
    fn discard_pcm(&self, inner: &StreamInner) -> Result<(), Error> {
        self.park_worker();
        let result = if inner.handle.state() != alsa::pcm::State::Setup {
            inner
                .handle
                .drop()
                .and_then(|_| inner.handle.prepare())
                .map_err(Error::from)
        } else {
            Ok(())
        };
        inner.control.unpark_worker();
        result
    }

    // PAUSE propagates through the link group like START does, so one call on capture pauses
    // both when linked; the explicit playback call only does work when unlinked.
    fn pause_duplex(&self, inner: &DuplexStreamInner) -> Result<(), Error> {
        let can_pause = inner.capture.handle.hw_params_current()?.can_pause()
            && inner.playback.handle.hw_params_current()?.can_pause();
        if can_pause {
            let result: Result<(), alsa::Error> = (|| {
                if inner.capture.handle.state() != alsa::pcm::State::Paused {
                    inner.capture.handle.pause(true)?;
                }
                if inner.playback.handle.state() != alsa::pcm::State::Paused {
                    inner.playback.handle.pause(true)?;
                }
                Ok(())
            })();
            // Some drivers advertise per-direction pause support that fails once the pair is
            // linked; fall through to the discard path below instead of surfacing that error.
            if result.is_ok() {
                return Ok(());
            }
        }

        self.park_worker();
        let capture_result = if inner.capture.handle.state() == alsa::pcm::State::Running {
            inner
                .capture
                .handle
                .drop()
                .and_then(|_| inner.capture.handle.prepare())
                .map_err(Error::from)
        } else {
            Ok(())
        };
        let playback_result = if inner.playback.handle.state() == alsa::pcm::State::Running {
            inner
                .playback
                .handle
                .drop()
                .and_then(|_| inner.playback.handle.prepare())
                .map_err(Error::from)
        } else {
            Ok(())
        };
        inner.control.unpark_worker();
        capture_result.and(playback_result)
    }

    // Discards capture and drains playback, per StreamTrait::stop's per-direction contract. Left
    // linked, capture.handle.start() in the next begin_duplex_playback() fails even after
    // preparing capture: a linked start() needs the whole group ready, and playback sits in
    // Setup until its own prepare() runs. Unlink first so each handle can be prepared and
    // started independently.
    fn stop_duplex(
        &self,
        inner: &DuplexStreamInner,
        timeout: Option<Duration>,
    ) -> Result<(), Error> {
        self.park_worker();
        if inner.linked.swap(false, Ordering::Relaxed) {
            inner.capture.handle.unlink().ok(); // best-effort
        }
        let capture_result = if inner.capture.handle.state() != alsa::pcm::State::Setup {
            inner
                .capture
                .handle
                .drop()
                .and_then(|_| inner.capture.handle.prepare())
                .map_err(Error::from)
        } else {
            Ok(())
        };
        let playback_result = drain_pcm(&inner.playback.handle, timeout);
        inner.control.unpark_worker();
        capture_result.and(playback_result)
    }

    // Signals the worker to exit: marks it dropping, unblocks it from acknowledge_park()
    // if parked, and wakes it from poll_for_period(). dropping must be set first so the
    // worker exits on re-entry rather than polling again.
    fn shutdown_worker(&self) {
        match &self.kind {
            StreamKind::Single(inner) => {
                inner.control.dropping.store(true, Ordering::Relaxed);
                inner.control.unpark_worker();
            }
            StreamKind::Duplex(inner) => {
                inner.control.dropping.store(true, Ordering::Relaxed);
                inner.control.unpark_worker();
            }
        }
        self.trigger.wakeup();
    }
}

// Drains a parked output PCM: caller holds exclusive access via park_worker()/unpark_worker().
fn drain_pcm(handle: &alsa::pcm::PCM, timeout: Option<Duration>) -> Result<(), Error> {
    if timeout == Some(Duration::ZERO) {
        handle.drop().ok();
        return handle.prepare().map_err(Into::into);
    }

    // Non-blocking drain: the PCM is opened non-blocking, so snd_pcm_drain returns EAGAIN
    // immediately. Poll the ALSA fds until drain completes or the deadline expires.
    let deadline = timeout.and_then(|t| Instant::now().checked_add(t));
    let mut fds = handle.get()?;
    let mut result: Result<(), Error> = Ok(());

    'drain: loop {
        match handle.drain() {
            Ok(()) => break,
            Err(e) if e.errno() == libc::EAGAIN => {
                let timeout_ms = match deadline {
                    None => POLL_INFINITE,
                    Some(deadline) => {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            handle.drop().ok();
                            break 'drain;
                        }
                        remaining.as_millis().min(i32::MAX as u128) as i32
                    }
                };
                match alsa::poll::poll(&mut fds, timeout_ms) {
                    Ok(0) => {
                        handle.drop().ok();
                        break 'drain;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        result = Err(e.into());
                        break;
                    }
                }
            }
            Err(e) => {
                result = Err(e.into());
                break;
            }
        }
    }

    // Leave PCM in PREPARED so the worker can resume normally.
    match handle.state() {
        alsa::pcm::State::Setup => {
            // Drain completed or drop-on-timeout succeeded.
            if let Err(e) = handle.prepare() {
                result = result.and(Err(e.into()));
            }
        }
        alsa::pcm::State::Draining => {
            // A poll error interrupted an in-progress drain; abort it.
            handle.drop().ok();
            if let Err(e) = handle.prepare() {
                result = result.and(Err(e.into()));
            }
        }
        _ => {} // XRun, Running, Disconnected: worker's own recovery handles it
    }

    result
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Unblock the worker in case the stream is dropped before start() was called.
        // Idempotent: no effect if the worker is already running.
        self.latch.release();
        self.shutdown_worker();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        match &self.kind {
            StreamKind::Single(inner) => {
                inner.control.draining.store(false, Ordering::Relaxed);
                self.latch.release(); // idempotent: no-op after first call
                inner.control.unpark_worker(); // resume if stop() left it parked; no-op otherwise
                match inner.handle.state() {
                    // Calling start() on an empty output buffer would trigger an immediate XRUN.
                    alsa::pcm::State::Prepared if inner.direction == DeviceDirection::Input => {
                        inner.handle.start()?;
                    }
                    alsa::pcm::State::Paused => {
                        inner.handle.pause(false)?;
                    }
                    // Guard against Setup in case prepare() in stop() failed silently.
                    alsa::pcm::State::Setup => {
                        inner.handle.prepare()?;
                        if inner.direction == DeviceDirection::Input {
                            inner.handle.start()?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            StreamKind::Duplex(inner) => {
                inner.control.draining.store(false, Ordering::Relaxed);
                self.latch.release();
                inner.control.unpark_worker();
                start_duplex(inner)
            }
        }
    }

    fn pause(&self) -> Result<(), Error> {
        match &self.kind {
            StreamKind::Single(inner) => {
                inner.control.draining.store(true, Ordering::Relaxed);
                self.suspend_pcm(inner)
            }
            StreamKind::Duplex(inner) => {
                inner.control.draining.store(true, Ordering::Relaxed);
                self.pause_duplex(inner)
            }
        }
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        match &self.kind {
            StreamKind::Single(inner) => {
                inner.control.draining.store(true, Ordering::Relaxed);

                if inner.direction != DeviceDirection::Output {
                    // Unlike pause(), stop() discards rather than preserves buffered samples.
                    return self.discard_pcm(inner);
                }

                self.park_worker();
                let result = drain_pcm(&inner.handle, timeout);
                inner.control.unpark_worker();
                result
            }
            StreamKind::Duplex(inner) => {
                inner.control.draining.store(true, Ordering::Relaxed);
                self.stop_duplex(inner, timeout)
            }
        }
    }

    fn now(&self) -> StreamInstant {
        match &self.kind {
            StreamKind::Single(inner) => {
                if inner.timestamp_mode != TimestampMode::CreationInstant {
                    if let Ok(status) = status_with_timestamp(&inner.handle, inner.timestamp_mode) {
                        return inner.callback_instant(&status);
                    }
                }
                let d = std::time::Instant::now().duration_since(inner.creation_instant);
                StreamInstant::new(d.as_secs(), d.subsec_nanos())
            }
            StreamKind::Duplex(inner) => {
                // Capture is the canonical clock, matching how process_duplex derives
                // DuplexCallbackInfo's capture-side timestamp.
                if inner.capture.timestamp_mode != TimestampMode::CreationInstant {
                    if let Ok(status) =
                        status_with_timestamp(&inner.capture.handle, inner.capture.timestamp_mode)
                    {
                        return callback_instant_for(
                            inner.capture.timestamp_mode,
                            inner.capture.creation_ts,
                            inner.creation_instant,
                            &status,
                        );
                    }
                }
                let d = std::time::Instant::now().duration_since(inner.creation_instant);
                StreamInstant::new(d.as_secs(), d.subsec_nanos())
            }
        }
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        match &self.kind {
            StreamKind::Single(inner) => Ok(inner.period_size as FrameCount),
            StreamKind::Duplex(inner) => Ok(inner.period_size as FrameCount),
        }
    }
}

/// Strategy for pre-filling an output buffer with the equilibrium value.
#[derive(Debug)]
pub(super) enum EquilibriumFill {
    /// Equilibrium is represented as a single repeating byte value.
    Byte(u8),
    /// A period-sized buffer pre-filled with the equilibrium value.
    Template(Box<[u8]>),
}

impl EquilibriumFill {
    /// Compute the equilibrium-fill strategy for the given sample format at stream creation.
    pub(super) fn new(sample_format: SampleFormat, period_bytes: usize) -> Self {
        if sample_format.is_int() || sample_format.is_float() {
            Self::Byte(0)
        } else if sample_format == SampleFormat::U8 {
            Self::Byte(U8_EQUILIBRIUM_BYTE)
        } else if sample_format.is_dsd() {
            Self::Byte(DSD_EQUILIBRIUM_BYTE)
        } else {
            // Multi-byte unsigned integer formats require a fill equal to the midpoint of their
            // range.
            debug_assert!(sample_format.is_uint());
            let mut template = vec![0u8; period_bytes].into_boxed_slice();
            fill_equilibrium(&mut template, sample_format);
            Self::Template(template)
        }
    }

    #[inline]
    pub(super) fn fill(&self, buffer: &mut [u8]) {
        match self {
            Self::Byte(b) => buffer.fill(*b),
            Self::Template(t) => buffer.copy_from_slice(t),
        }
    }
}

// A zero get_htstamp() at prepare time indicates the device does not support hardware
// timestamps (e.g. PulseAudio ALSA plugin). Related:
// https://bugs.freedesktop.org/show_bug.cgi?id=88503
pub(super) fn timestamp_mode_for(
    hw_params: &alsa::pcm::HwParams<'_>,
    creation_ts: alsa::timespec,
) -> TimestampMode {
    if creation_ts.tv_sec == 0 && creation_ts.tv_nsec == 0 {
        TimestampMode::CreationInstant
    } else if hw_params.supports_audio_ts_type(alsa::pcm::AudioTstampType::LinkSynchronized) {
        TimestampMode::AudioLink
    } else {
        TimestampMode::SystemClock
    }
}

// How callback timestamps are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimestampMode {
    // Hardware timestamps are unavailable (e.g. PulseAudio ALSA plugin returns zero htstamp).
    // Timestamps are monotonic elapsed time since stream creation, sourced from Instant::now().
    CreationInstant,

    // The kernel records the monotonic clock at each DMA interrupt in htstamp.
    // Subtracting creation_ts (same clock, captured at prepare time) gives elapsed time
    // since stream creation. Uses CLOCK_MONOTONIC_RAW when available, CLOCK_MONOTONIC otherwise.
    SystemClock,

    // The hardware maps the audio sample counter to CLOCK_MONOTONIC_RAW via TSC
    // cross-timestamps (LinkSynchronized), giving a timestamp that tracks the actual audio
    // clock rather than DMA interrupt delivery time. Higher fidelity than SystemClock.
    AudioLink,
}

// Park/drop plumbing shared by StreamInner and DuplexStreamInner, giving StreamTrait
// exclusive worker access for pause/stop/drain regardless of handle count.
#[derive(Debug, Default)]
pub(super) struct WorkerControl {
    // Set when the worker should stop polling, e.g. after a device disconnect.
    pub(super) dropping: AtomicBool,

    // Whether the user callback is currently suppressed.
    pub(super) draining: AtomicBool,

    // Set by stop() to request the worker pause for exclusive PCM access during drain.
    pub(super) parked: AtomicBool,
    park: Notify,
}

impl WorkerControl {
    // Pauses the worker at its next loop iteration and waits for acknowledgment, or returns
    // early if it already exited. Caller holds exclusive PCM access until unpark_worker().
    pub(super) fn park_worker(&self) {
        self.parked.store(true, Ordering::Relaxed);
        let (lock, cvar) = &self.park;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        // Exit if the worker acknowledged the park OR if the worker has exited (dropping=true).
        while !*guard && !self.dropping.load(Ordering::Relaxed) {
            guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
    }

    // Acknowledges a pending park, then sleeps until unpark_worker() is called.
    pub(super) fn acknowledge_park(&self) {
        let (lock, cvar) = &self.park;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        *guard = true;
        cvar.notify_one();
        while self.parked.load(Ordering::Relaxed) {
            guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
        *guard = false;
    }

    // Marks the stream dead and wakes any thread blocked in park_worker(), so an exit
    // other than a normal drop doesn't hang it.
    pub(super) fn signal_worker_exit(&self) {
        self.dropping.store(true, Ordering::Relaxed);
        let (lock, cvar) = &self.park;
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        cvar.notify_one();
    }

    // Releases the park: clears parked and wakes the worker from acknowledge_park().
    pub(super) fn unpark_worker(&self) {
        let (lock, cvar) = &self.park;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        *guard = false;
        self.parked.store(false, Ordering::Relaxed);
        drop(guard);
        cvar.notify_one();
    }
}

#[derive(Debug)]
pub(super) struct StreamInner {
    // Controls the worker thread's lifecycle and pause/drain state.
    pub(super) control: WorkerControl,

    // Stream direction.
    pub(super) direction: DeviceDirection,

    // The ALSA handle.
    pub(super) handle: alsa::pcm::PCM,

    // Format of the samples.
    pub(super) sample_format: SampleFormat,

    // Sample rate of the stream.
    pub(super) sample_rate: SampleRate,

    // Cached values for performance in audio callback hot path.
    pub(super) frame_size: usize,
    pub(super) period_size: usize,
    pub(super) period_samples: usize,
    // Only used for Output direction.
    pub(super) equilibrium: Option<EquilibriumFill>,

    // How callback timestamps are produced.
    pub(super) timestamp_mode: TimestampMode,

    // htstamp value from the status query at prepare() time.
    // Used as the creation-time anchor for SystemClock and AudioLink calculations.
    pub(super) creation_ts: alsa::timespec,

    // Monotonic instant captured at stream creation. Timestamp origin for CreationInstant
    // mode and last-resort fallback if the status query in now() fails.
    pub(super) creation_instant: std::time::Instant,

    // Xrun pending delivery to the data callback.
    pub(super) pending_xrun: AtomicBool,

    // Keep ALSA context alive to prevent premature ALSA config cleanup.
    pub(super) _context: Arc<AlsaContext>,
}

// Assume that the ALSA library is built with thread safe option.
unsafe impl Sync for StreamInner {}

impl StreamInner {
    #[inline]
    pub(super) fn callback_instant(&self, status: &alsa::pcm::Status) -> StreamInstant {
        callback_instant_for(
            self.timestamp_mode,
            self.creation_ts,
            self.creation_instant,
            status,
        )
    }

    #[cfg(feature = "realtime")]
    pub(super) fn is_rt_eligible(&self) -> bool {
        pcm_is_rt_eligible(&self.handle)
    }
}

#[derive(Debug)]
pub(super) struct DuplexCaptureState {
    pub(super) handle: alsa::pcm::PCM,
    pub(super) sample_format: SampleFormat,
    pub(super) frame_size: usize,
    pub(super) period_samples: usize,
    pub(super) timestamp_mode: TimestampMode,
    pub(super) creation_ts: alsa::timespec,
}

#[derive(Debug)]
pub(super) struct DuplexPlaybackState {
    pub(super) handle: alsa::pcm::PCM,
    pub(super) sample_format: SampleFormat,
    pub(super) frame_size: usize,
    pub(super) period_samples: usize,
    pub(super) timestamp_mode: TimestampMode,
    pub(super) creation_ts: alsa::timespec,
    pub(super) equilibrium: EquilibriumFill,
}

#[derive(Debug)]
pub(super) struct DuplexStreamInner {
    pub(super) control: WorkerControl,

    pub(super) capture: DuplexCaptureState,
    pub(super) playback: DuplexPlaybackState,

    pub(super) sample_rate: SampleRate,
    pub(super) period_size: usize,

    // Ties capture and playback via snd_pcm_link(). begin_duplex_playback() retries when false.
    // Recovery and pause leave it alone; ALSA doesn't document those as severing a link.
    pub(super) linked: AtomicBool,

    pub(super) creation_instant: Instant,
    pub(super) pending_xrun: AtomicBool,
    pub(super) _context: Arc<AlsaContext>,
}

// Assume that the ALSA library is built with thread safe option.
unsafe impl Sync for DuplexStreamInner {}

impl DuplexStreamInner {
    #[cfg(feature = "realtime")]
    pub(super) fn is_rt_eligible(&self) -> bool {
        pcm_is_rt_eligible(&self.capture.handle) && pcm_is_rt_eligible(&self.playback.handle)
    }
}

#[cfg(feature = "realtime")]
fn pcm_is_rt_eligible(handle: &alsa::pcm::PCM) -> bool {
    use alsa_sys::*;
    // SAFETY: `alsa::pcm::PCM` is `pub struct PCM(*mut snd_pcm_t, Cell<bool>)`. The crate
    // does not expose a public `as_ptr()`, but we can cast and read from it.
    // TODO: replace with `handle.as_ptr()` once alsa-rs exposes it publicly.
    let raw = unsafe {
        (handle as *const alsa::pcm::PCM)
            .cast::<*mut snd_pcm_t>()
            .read()
    };
    let pcm_type = unsafe { snd_pcm_type(raw) };

    // Only attempt RT promotion for types known not to spin and not to chain to a
    // server-backed backend. Therefore, we exclude:
    // - NULL: always-ready poll() spins and exhausts RLIMIT_RTTIME, causing SIGXCPU.
    // - IOPLUG/EXTPLUG: may route to PulseAudio, causing priority inversion and SIGXCPU.
    // - HOOKS, SOFTVOL, PLUG, RATE, ROUTE, COPY: that can chain to either of the above.
    matches!(
        pcm_type,
        SND_PCM_TYPE_HW
            | SND_PCM_TYPE_LINEAR
            | SND_PCM_TYPE_ALAW
            | SND_PCM_TYPE_MULAW
            | SND_PCM_TYPE_ADPCM
            | SND_PCM_TYPE_LINEAR_FLOAT
            | SND_PCM_TYPE_IEC958
    )
}
