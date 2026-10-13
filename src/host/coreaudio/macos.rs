use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex, Weak,
};

use coreaudio::audio_unit::{AudioUnit, Scope};
use objc2_core_audio::{
    kAudioDeviceProcessorOverload, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyNominalSampleRate, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioDeviceID, AudioObjectID, AudioObjectPropertyAddress,
};
use property_listener::AudioObjectPropertyListener;

pub use self::enumerate::{default_input_device, default_output_device, Devices};
use super::{asbd_from_config, check_os_status, host_time_to_stream_instant, OSStatus};
use crate::{
    host::{coreaudio::macos::loopback::LoopbackDevice, emit_error, latch::Latch, try_emit_error},
    traits::{HostTrait, StreamTrait},
    Error, ErrorKind, FrameCount, ResultExt, StreamInstant,
};

mod device;
pub mod enumerate;
mod loopback;
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

/// Registers a `kAudioDeviceProcessorOverload` listener on `device_id`, reporting `Xrun` through
/// `error_callback` on each firing (overload notifications fire on the RT thread).
fn spawn_overload_listener(
    device_id: AudioDeviceID,
    error_callback: &Arc<Mutex<ErrorCallback>>,
) -> Result<mpsc::Sender<()>, Error> {
    let error_callback = error_callback.clone();
    spawn_property_listener_thread(
        device_id,
        AudioObjectPropertyAddress {
            mSelector: kAudioDeviceProcessorOverload,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        },
        move || {
            let _ = try_emit_error(&error_callback, ErrorKind::Xrun.into());
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
    ) -> Result<Self, Error> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (disconnect_tx, disconnect_rx) = mpsc::channel::<Error>();
        let (ready_tx, ready_rx) = mpsc::channel();

        // Spawn a dedicated thread to own all listeners. CoreAudio requires that
        // AudioObjectPropertyListeners are added and removed on the same thread.
        let disconnect_tx_alive = disconnect_tx.clone();
        let disconnect_tx_rate = disconnect_tx;
        let error_callback_overload = error_callback.clone();
        std::thread::spawn(move || {
            let alive_address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyDeviceIsAlive,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };
            let alive_listener =
                AudioObjectPropertyListener::new(device_id, alive_address, move || {
                    let _ = disconnect_tx_alive.send(Error::with_message(
                        ErrorKind::DeviceNotAvailable,
                        "Device disconnected",
                    ));
                });

            let rate_address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyNominalSampleRate,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };
            let rate_listener =
                AudioObjectPropertyListener::new(device_id, rate_address, move || {
                    let _ = disconnect_tx_rate.send(Error::with_message(
                        ErrorKind::StreamInvalidated,
                        "Device sample rate changed",
                    ));
                });

            // Overload notifications fire on the RT thread.
            let overload_address = AudioObjectPropertyAddress {
                mSelector: kAudioDeviceProcessorOverload,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };
            let overload_listener =
                AudioObjectPropertyListener::new(device_id, overload_address, move || {
                    let _ = try_emit_error(&error_callback_overload, ErrorKind::Xrun.into());
                });

            match (alive_listener, rate_listener, overload_listener) {
                (Ok(_alive), Ok(_rate), Ok(_overload)) => {
                    let _ = ready_tx.send(Ok(()));
                    // Block until the stream is dropped; listeners are removed on drop.
                    let _ = shutdown_rx.recv();
                }
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
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
        let waiter = latch.waiter();

        let handle = std::thread::Builder::new()
            .name("cpal-coreaudio-disconnect".into())
            .spawn(move || {
                // If the Latch is dropped without being released (error path), exit cleanly.
                if !waiter.wait() {
                    return;
                }
                while let Ok(err) = disconnect_rx.recv() {
                    if let Some(stream_arc) = stream_weak.upgrade() {
                        if let Ok(mut stream_inner) = stream_arc.try_lock() {
                            let _ = stream_inner.pause();
                        }
                        emit_error(&error_callback, err);
                    } else {
                        break;
                    }
                }
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("Failed to spawn disconnect thread: {e}"),
                )
            })?;

        latch.add_thread(handle.thread().clone());
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
}

impl DefaultOutputMonitor {
    fn new(
        stream_weak: Weak<Mutex<StreamInner>>,
        error_callback: Arc<Mutex<ErrorCallback>>,
        latency_refresh: Option<(Arc<AtomicUsize>, Scope)>,
    ) -> Result<Self, Error> {
        let (change_tx, change_rx) = mpsc::channel::<()>();
        let shutdown_tx = spawn_property_listener_thread(
            kAudioObjectSystemObject as AudioObjectID,
            AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDefaultOutputDevice,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            },
            move || {
                let _ = change_tx.send(());
            },
        )?;

        // The overload (xrun) listener targets a specific device, so it must be re-registered
        // against whatever device is current whenever the default output reroutes. Held only to
        // shut down the previous listener thread on drop when reassigned below.
        let overload_error_callback = error_callback.clone();
        let mut _overload_shutdown_tx = default_output_device()
            .map(|device| spawn_overload_listener(device.audio_device_id, &overload_error_callback))
            .transpose()?;

        let mut latch = Latch::new();
        let waiter = latch.waiter();

        let handle = std::thread::Builder::new()
            .name("cpal-coreaudio-default-output".into())
            .spawn(move || {
                if !waiter.wait() {
                    return;
                }
                while let Ok(()) = change_rx.recv() {
                    let Some(stream) = stream_weak.upgrade() else {
                        break;
                    };
                    match default_output_device() {
                        None => {
                            _overload_shutdown_tx = None;
                            if let Ok(mut inner) = stream.try_lock() {
                                let _ = inner.pause();
                            }
                            emit_error(
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
                            if let Some((frames, scope)) = &latency_refresh {
                                if let Ok(inner) = stream.lock() {
                                    let depth =
                                        device::get_device_buffer_frame_size(&inner.audio_unit)
                                            .ok()
                                            .map_or(0, |buffer| {
                                                buffer
                                                    + device::get_device_extra_latency_frames(
                                                        &inner.audio_unit,
                                                        *scope,
                                                    )
                                            });
                                    frames.store(depth, Ordering::Relaxed);
                                }
                            }
                            _overload_shutdown_tx = spawn_overload_listener(
                                device.audio_device_id,
                                &overload_error_callback,
                            )
                            .ok();
                            emit_error(
                                &error_callback,
                                Error::with_message(
                                    ErrorKind::DeviceChanged,
                                    "default output device changed",
                                ),
                            );
                        }
                    }
                }
            })
            .map_err(|e| {
                Error::with_message(
                    ErrorKind::ResourceExhausted,
                    format!("failed to spawn default-output monitor thread: {e}"),
                )
            })?;

        latch.add_thread(handle.thread().clone());
        Ok(DefaultOutputMonitor {
            latch,
            _shutdown_tx: shutdown_tx,
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
    fn play(&mut self) -> Result<(), Error> {
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
}

impl Stream {
    fn new(inner: Arc<Mutex<StreamInner>>, monitor: Box<dyn Monitor>) -> Self {
        Self { inner, monitor }
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
    fn play(&self) -> Result<(), Error> {
        self.inner
            .lock()
            .map_err(|_| Error::with_message(ErrorKind::StreamInvalidated, "Stream lock poisoned"))?
            .play()
    }

    fn pause(&self) -> Result<(), Error> {
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
        default_host,
        traits::{DeviceTrait, HostTrait, StreamTrait},
        InputCallbackInfo, OutputCallbackInfo, Sample,
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
        stream.play().unwrap();
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
                move |data: &[f32], _: &InputCallbackInfo| {
                    // react to stream events and read or write stream data here.
                    println!("Got data: {:?}", &data[..25]);
                },
                move |err| println!("Error: {err}"),
                None, // None=blocking, Some(Duration)=timeout
            )
            .unwrap();
        stream.play().unwrap();
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
                move |data: &[f32], _: &InputCallbackInfo| {
                    // react to stream events and read or write stream data here.
                    println!("Got data: {:?}", &data[..25]);
                },
                move |err| println!("Error: {err}"),
                None, // None=blocking, Some(Duration)=timeout
            )
            .unwrap();
        stream.play().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    fn write_silence<T: Sample>(data: &mut [T], _: &OutputCallbackInfo) {
        for sample in data.iter_mut() {
            *sample = Sample::EQUILIBRIUM;
        }
    }
}
