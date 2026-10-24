use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use coreaudio::audio_unit::{AudioUnit, Scope};
use objc2_core_audio::{
    AudioDeviceID, AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertySelector,
    kAudioDeviceProcessorOverload, kAudioDevicePropertyBufferFrameSize,
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyNominalSampleRate,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
};
use property_listener::AudioObjectPropertyListener;

pub use self::enumerate::{Devices, default_input_device, default_output_device};
use super::{OSStatus, asbd_from_config, check_os_status, host_time_to_stream_instant};
use crate::{
    Error, ErrorKind, FrameCount, ResultExt, SampleRate, StreamInstant,
    host::{
        coreaudio::macos::loopback::LoopbackDevice, emit_error, frames_to_duration, latch::Latch,
        wait_for_drain,
    },
    traits::{HostTrait, StreamTrait},
};

mod device;
pub mod enumerate;
mod format;
mod loopback;
mod property;
mod property_listener;
pub use device::Device;

/// CoreAudio host, the default host on macOS.
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, Error> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume coreaudio is always available
        true
    }

    fn devices(&self) -> Result<Self::Devices, Error> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
}

/// Type alias for the error callback to reduce complexity
type ErrorCallback = dyn FnMut(Error) + Send;

/// The cached buffer depth to refresh when the device's buffer frame size changes, and the scope
/// it was measured in.
type LatencyRefresh = (Arc<AtomicUsize>, Scope);

/// What a device's property listeners report to its delivery thread.
enum MonitorEvent {
    /// The device went away, or changed such that the stream is no longer valid.
    Lost(Error),
    /// The device's buffer frame size changed, so the cached buffer depth is stale.
    LatencyChanged,
}

/// Device-scoped property address, the shape every listener here uses.
fn device_address(selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Recomputes the buffer depth for `refresh` from the stream's current device.
fn refresh_latency(stream: &Mutex<StreamInner>, refresh: &LatencyRefresh) {
    let (frames, scope) = refresh;
    if let Ok(inner) = stream.lock() {
        frames.store(
            device::device_latency_frames(&inner.audio_unit, *scope),
            Ordering::Relaxed,
        );
    }
}

/// Spawns a dedicated thread that registers a single property listener, calling `on_change` on
/// each firing. The listener is deregistered when the returned `Sender<()>` is dropped.
fn spawn_property_listener_thread<F>(
    object_id: AudioObjectID,
    address: AudioObjectPropertyAddress,
    on_change: F,
) -> Result<mpsc::Sender<()>, Error>
where
    F: FnMut() + Send + 'static,
{
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel();

    std::thread::spawn(move || {
        let listener = AudioObjectPropertyListener::new(object_id, address, on_change);
        match listener {
            Ok(_l) => {
                let _ = ready_tx.send(Ok(()));
                let _ = shutdown_rx.recv();
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        }
    });

    ready_rx.recv().map_err(|_| {
        Error::with_message(
            ErrorKind::StreamInvalidated,
            "property listener thread terminated unexpectedly",
        )
    })??;

    Ok(shutdown_tx)
}

/// Spawns the delivery thread shared by both monitors.
///
/// It waits for the owning `Stream` to reach the caller, then applies each event until the stream
/// is dropped or every sender is gone. Property listeners must not call back into CoreAudio, so
/// they only post events and this thread does the work.
fn spawn_delivery_thread<E, F>(
    name: &str,
    latch: &mut Latch,
    events: mpsc::Receiver<E>,
    stream_weak: Weak<Mutex<StreamInner>>,
    mut on_event: F,
) -> Result<(), Error>
where
    E: Send + 'static,
    F: FnMut(E, &Arc<Mutex<StreamInner>>) + Send + 'static,
{
    let waiter = latch.waiter();
    let handle = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            // If the Latch is dropped without being released (error path), exit cleanly.
            if !waiter.wait() {
                return;
            }
            while let Ok(event) = events.recv() {
                let Some(stream) = stream_weak.upgrade() else {
                    break;
                };
                on_event(event, &stream);
            }
        })
        .map_err(|e| {
            Error::with_message(
                ErrorKind::ResourceExhausted,
                format!("failed to spawn {name} thread: {e}"),
            )
        })?;
    latch.add_thread(handle.thread().clone());
    Ok(())
}

/// Halts the stream and reports why, for the cases where it cannot keep running.
fn report_lost(
    stream: &Mutex<StreamInner>,
    error_callback: &Arc<Mutex<ErrorCallback>>,
    err: Error,
) {
    if let Ok(mut inner) = stream.try_lock() {
        let _ = inner.pause();
    }
    emit_error(error_callback, err);
}

/// Registers an overload listener for `device_id`. These fire on the RT thread, so the callback
/// only sets a flag.
fn spawn_overload_listener(
    device_id: AudioDeviceID,
    pending_xrun: Arc<AtomicBool>,
) -> Result<mpsc::Sender<()>, Error> {
    spawn_property_listener_thread(
        device_id,
        device_address(kAudioDeviceProcessorOverload),
        move || {
            pending_xrun.store(true, Ordering::Relaxed);
        },
    )
}

/// A device monitor that can signal when the owning `Stream` handle has been returned to the
/// caller, allowing the delivery thread to start processing events.
pub(super) trait Monitor: Send + Sync {
    /// Unblocks the delivery thread. Called after `Stream::new()` and from `Stream::drop()`.
    fn signal_ready(&self);
}

/// Manages device disconnection listener on a dedicated thread to ensure the
/// AudioObjectPropertyListener is always created and dropped on the same thread.
/// This avoids potential threading issues with CoreAudio APIs.
///
/// When a device disconnects, this manager:
/// 1. Attempts to pause the stream to stop audio I/O
/// 2. Calls the error callback with `ErrorKind::DeviceNotAvailable`
///
/// The dedicated thread architecture ensures `Stream` can implement `Send`.
struct DisconnectManager {
    latch: Latch,
    _shutdown_tx: mpsc::Sender<()>,
}

impl DisconnectManager {
    fn new(
        device_id: AudioDeviceID,
        stream_weak: Weak<Mutex<StreamInner>>,
        error_callback: Arc<Mutex<ErrorCallback>>,
        latency_refresh: LatencyRefresh,
        pending_xrun: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (disconnect_tx, disconnect_rx) = mpsc::channel::<MonitorEvent>();
        let (ready_tx, ready_rx) = mpsc::channel();

        // Spawn a dedicated thread to own all listeners. CoreAudio requires that
        // AudioObjectPropertyListeners are added and removed on the same thread.
        let disconnect_tx_alive = disconnect_tx.clone();
        let disconnect_tx_rate = disconnect_tx.clone();
        let disconnect_tx_buffer = disconnect_tx;
        std::thread::spawn(move || {
            let alive_listener = AudioObjectPropertyListener::new(
                device_id,
                device_address(kAudioDevicePropertyDeviceIsAlive),
                move || {
                    let _ = disconnect_tx_alive.send(MonitorEvent::Lost(Error::with_message(
                        ErrorKind::DeviceNotAvailable,
                        "Device disconnected",
                    )));
                },
            );

            let rate_listener = AudioObjectPropertyListener::new(
                device_id,
                device_address(kAudioDevicePropertyNominalSampleRate),
                move || {
                    let _ = disconnect_tx_rate.send(MonitorEvent::Lost(Error::with_message(
                        ErrorKind::StreamInvalidated,
                        "Device sample rate changed",
                    )));
                },
            );

            // Device-global on macOS: another process changing it resizes our IO buffer too.
            let buffer_size_listener = AudioObjectPropertyListener::new(
                device_id,
                device_address(kAudioDevicePropertyBufferFrameSize),
                move || {
                    let _ = disconnect_tx_buffer.send(MonitorEvent::LatencyChanged);
                },
            );

            // Overload notifications fire on the RT thread.
            let overload_listener = AudioObjectPropertyListener::new(
                device_id,
                device_address(kAudioDeviceProcessorOverload),
                move || {
                    pending_xrun.store(true, Ordering::Relaxed);
                },
            );

            match (
                alive_listener,
                rate_listener,
                buffer_size_listener,
                overload_listener,
            ) {
                (Ok(_alive), Ok(_rate), Ok(_buffer), Ok(_overload)) => {
                    let _ = ready_tx.send(Ok(()));
                    // Block until the stream is dropped; listeners are removed on drop.
                    let _ = shutdown_rx.recv();
                }
                (Err(e), ..) | (_, Err(e), ..) | (_, _, Err(e), _) | (_, _, _, Err(e)) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        });

        ready_rx.recv().map_err(|_| {
            Error::with_message(
                ErrorKind::StreamInvalidated,
                "Stream monitor terminated unexpectedly",
            )
        })??;

        let mut latch = Latch::new();
        spawn_delivery_thread(
            "cpal-coreaudio-disconnect",
            &mut latch,
            disconnect_rx,
            stream_weak,
            move |event, stream| match event {
                MonitorEvent::Lost(err) => report_lost(stream, &error_callback, err),
                MonitorEvent::LatencyChanged => refresh_latency(stream, &latency_refresh),
            },
        )?;

        Ok(DisconnectManager {
            latch,
            _shutdown_tx: shutdown_tx,
        })
    }
}

impl Monitor for DisconnectManager {
    fn signal_ready(&self) {
        self.latch.release();
    }
}

/// Manages the system default output device change listener on a dedicated thread.
///
/// When the system default output device changes:
/// - If a new valid default exists, AudioUnit reroutes and `DeviceChanged` is reported.
/// - If there is no new default, the stream is paused and `DeviceNotAvailable` is reported.
struct DefaultOutputMonitor {
    latch: Latch,
    _shutdown_tx: mpsc::Sender<()>,
    // Both are held here rather than in the delivery thread: a sender that thread owned, directly
    // or through a listener it kept alive, would hold the event channel open so its loop could
    // never end. Dropping these is what lets it exit.
    _event_tx: Arc<mpsc::Sender<DefaultOutputEvent>>,
    buffer_size_listener: BufferSizeListener,
}

/// Shutdown handle for the buffer-size listener, re-registered by the delivery thread on reroute.
type BufferSizeListener = Arc<Mutex<Option<mpsc::Sender<()>>>>;

/// What the default-output listeners report to that monitor's delivery thread.
enum DefaultOutputEvent {
    /// The system default output device changed.
    DeviceChanged,
    /// The current device's buffer frame size changed, so the cached buffer depth is stale.
    LatencyChanged,
}

/// Registers a buffer-size listener for `device_id`, reporting through `event_tx`.
fn spawn_buffer_size_listener(
    device_id: AudioDeviceID,
    event_tx: mpsc::Sender<DefaultOutputEvent>,
) -> Result<mpsc::Sender<()>, Error> {
    spawn_property_listener_thread(
        device_id,
        device_address(kAudioDevicePropertyBufferFrameSize),
        move || {
            let _ = event_tx.send(DefaultOutputEvent::LatencyChanged);
        },
    )
}

/// Replaces the buffer-size listener, dropping the previous one so its thread exits.
fn set_buffer_size_listener(listener: &BufferSizeListener, next: Option<mpsc::Sender<()>>) {
    *listener.lock().unwrap_or_else(|e| e.into_inner()) = next;
}

impl Drop for DefaultOutputMonitor {
    fn drop(&mut self) {
        // Release the listener before `_event_tx`, so every sender is gone and the delivery
        // thread's loop ends.
        set_buffer_size_listener(&self.buffer_size_listener, None);
    }
}

impl DefaultOutputMonitor {
    fn new(
        stream_weak: Weak<Mutex<StreamInner>>,
        error_callback: Arc<Mutex<ErrorCallback>>,
        latency_refresh: LatencyRefresh,
        pending_xrun: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let (change_tx, change_rx) = mpsc::channel::<DefaultOutputEvent>();
        let event_tx = Arc::new(change_tx);
        let shutdown_tx = {
            let change_tx = mpsc::Sender::clone(&event_tx);
            spawn_property_listener_thread(
                kAudioObjectSystemObject as AudioObjectID,
                device_address(kAudioHardwarePropertyDefaultOutputDevice),
                move || {
                    let _ = change_tx.send(DefaultOutputEvent::DeviceChanged);
                },
            )?
        };

        // These listeners target a specific device, so they must be re-registered against
        // whatever device is current whenever the default output reroutes.
        // Held only to shut down the previous listener thread on drop when reassigned below.
        let buffer_size_listener: BufferSizeListener = Arc::new(Mutex::new(None));
        let mut _overload_shutdown_tx = match default_output_device() {
            Some(device) => {
                set_buffer_size_listener(
                    &buffer_size_listener,
                    Some(spawn_buffer_size_listener(
                        device.audio_device_id,
                        mpsc::Sender::clone(&event_tx),
                    )?),
                );
                Some(spawn_overload_listener(
                    device.audio_device_id,
                    pending_xrun.clone(),
                )?)
            }
            None => None,
        };

        // Weak, so the thread can hand a sender to a new listener without owning one itself.
        let event_tx_weak = Arc::downgrade(&event_tx);
        let buffer_size_listener_thread = buffer_size_listener.clone();

        let mut latch = Latch::new();
        spawn_delivery_thread(
            "cpal-coreaudio-default-output",
            &mut latch,
            change_rx,
            stream_weak,
            move |event, stream| {
                if matches!(event, DefaultOutputEvent::LatencyChanged) {
                    // Same device, resized buffer: refresh the depth only. The listeners still
                    // target the right device, and no route changed to report.
                    refresh_latency(stream, &latency_refresh);
                    return;
                }
                match default_output_device() {
                    None => {
                        _overload_shutdown_tx = None;
                        set_buffer_size_listener(&buffer_size_listener_thread, None);
                        report_lost(
                            stream,
                            &error_callback,
                            Error::with_message(
                                ErrorKind::DeviceNotAvailable,
                                "no default output device",
                            ),
                        );
                    }
                    Some(device) => {
                        // DefaultOutput AudioUnit rerouted automatically: recompute and notify
                        // the buffer depth for the new device.
                        refresh_latency(stream, &latency_refresh);
                        _overload_shutdown_tx =
                            spawn_overload_listener(device.audio_device_id, pending_xrun.clone())
                                .ok();
                        // Skipped once the monitor is dropped: there is nothing left to notify.
                        set_buffer_size_listener(
                            &buffer_size_listener_thread,
                            event_tx_weak.upgrade().and_then(|event_tx| {
                                spawn_buffer_size_listener(
                                    device.audio_device_id,
                                    mpsc::Sender::clone(&event_tx),
                                )
                                .ok()
                            }),
                        );
                        emit_error(
                            &error_callback,
                            Error::with_message(
                                ErrorKind::DeviceChanged,
                                "default output device changed",
                            ),
                        );
                    }
                }
            },
        )?;

        Ok(DefaultOutputMonitor {
            latch,
            _shutdown_tx: shutdown_tx,
            _event_tx: event_tx,
            buffer_size_listener,
        })
    }
}

impl Monitor for DefaultOutputMonitor {
    fn signal_ready(&self) {
        self.latch.release();
    }
}

struct StreamInner {
    playing: bool,
    audio_unit: AudioUnit,
    // Track the device with which the audio unit was spawned
    _device_id: AudioDeviceID,
    /// Manage the lifetime of the aggregate device used for loopback recording
    _loopback_device: Option<LoopbackDevice>,
}

impl StreamInner {
    fn start(&mut self) -> Result<(), Error> {
        if !self.playing {
            self.audio_unit
                .start()
                .context("Failed to start audio unit")?;
            self.playing = true;
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Error> {
        if self.playing {
            self.audio_unit
                .stop()
                .context("Failed to stop audio unit")?;
            self.playing = false;
        }
        Ok(())
    }
}

pub struct Stream {
    inner: Arc<Mutex<StreamInner>>,
    monitor: Box<dyn Monitor>,
    draining: Arc<AtomicBool>,
    // Shared with the reroute monitor so stop() sees the current depth. Zero on capture: no drain.
    drain_frames: Arc<AtomicUsize>,
    sample_rate: SampleRate,
}

impl Stream {
    fn new(
        inner: Arc<Mutex<StreamInner>>,
        monitor: Box<dyn Monitor>,
        draining: Arc<AtomicBool>,
        drain_frames: Arc<AtomicUsize>,
        sample_rate: SampleRate,
    ) -> Self {
        Self {
            inner,
            monitor,
            draining,
            drain_frames,
            sample_rate,
        }
    }

    fn signal_ready(&self) {
        self.monitor.signal_ready();
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Unblock monitor delivery threads if the stream is dropped early.
        self.monitor.signal_ready();
    }
}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        self.draining.store(false, Ordering::Relaxed);
        self.inner
            .lock()
            .map_err(|_| Error::with_message(ErrorKind::StreamInvalidated, "Stream lock poisoned"))?
            .start()
    }

    fn pause(&self) -> Result<(), Error> {
        self.inner
            .lock()
            .map_err(|_| Error::with_message(ErrorKind::StreamInvalidated, "Stream lock poisoned"))?
            .pause()
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        self.draining.store(true, Ordering::Relaxed);

        wait_for_drain(
            frames_to_duration(
                self.drain_frames.load(Ordering::Relaxed) as FrameCount,
                self.sample_rate,
            ),
            timeout,
        );

        self.inner
            .lock()
            .map_err(|_| Error::with_message(ErrorKind::StreamInvalidated, "Stream lock poisoned"))?
            .pause()
    }

    fn now(&self) -> StreamInstant {
        let m_host_time = unsafe { mach2::mach_time::mach_absolute_time() };
        host_time_to_stream_instant(m_host_time).expect("mach_timebase_info failed")
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        let stream = self.inner.lock().map_err(|_| {
            Error::with_message(ErrorKind::StreamInvalidated, "Stream lock poisoned")
        })?;
        device::get_device_buffer_frame_size(&stream.audio_unit)
            .map(|size| size as FrameCount)
            .context("Failed to get buffer frame size")
    }
}

#[cfg(test)]
mod test {
    use crate::{
        CallbackInfo, Sample, default_host,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    };

    #[test]
    fn test_play() {
        let host = default_host();
        let device = host.default_output_device().unwrap();

        let mut supported_configs_range = device.supported_output_configs().unwrap();
        let supported_config = supported_configs_range
            .next()
            .unwrap()
            .with_max_sample_rate();
        let config = supported_config.config();

        let stream = device
            .build_output_stream(
                config,
                write_silence::<f32>,
                move |err| println!("Error: {err}"),
                None, // None=blocking, Some(Duration)=timeout
            )
            .unwrap();
        stream.start().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    #[test]
    fn test_record() {
        let host = default_host();
        let device = host.default_input_device().unwrap();
        println!("Device: {:?}", device.description());

        let mut supported_configs_range = device.supported_input_configs().unwrap();
        println!("Supported configs:");
        for config in supported_configs_range.clone() {
            println!("{:?}", config)
        }
        let supported_config = supported_configs_range
            .next()
            .unwrap()
            .with_max_sample_rate();
        let config = supported_config.config();

        let stream = device
            .build_input_stream(
                config,
                move |data: &[f32], _: &CallbackInfo| {
                    // react to stream events and read or write stream data here.
                    println!("Got data: {:?}", &data[..25]);
                },
                move |err| println!("Error: {err}"),
                None, // None=blocking, Some(Duration)=timeout
            )
            .unwrap();
        stream.start().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    #[test]
    fn test_record_output() {
        if std::env::var("CI").is_ok() {
            println!("Skipping test_record_output in CI environment due to permissions");
            return;
        }

        let host = default_host();
        let device = host.default_output_device().unwrap();

        let mut supported_configs_range = device.supported_output_configs().unwrap();
        let supported_config = supported_configs_range
            .next()
            .unwrap()
            .with_max_sample_rate();
        let config = supported_config.config();

        println!("Building input stream");
        let stream = device
            .build_input_stream(
                config,
                move |data: &[f32], _: &CallbackInfo| {
                    // react to stream events and read or write stream data here.
                    println!("Got data: {:?}", &data[..25]);
                },
                move |err| println!("Error: {err}"),
                None, // None=blocking, Some(Duration)=timeout
            )
            .unwrap();
        stream.start().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    fn write_silence<T: Sample>(data: &mut [T], _: &CallbackInfo) {
        for sample in data.iter_mut() {
            *sample = Sample::EQUILIBRIUM;
        }
    }
}
