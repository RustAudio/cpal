#![allow(deprecated)]
use std::sync::{mpsc, Arc, Mutex, Weak};

use coreaudio::audio_unit::AudioUnit;
use objc2_core_audio::{
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyNominalSampleRate,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, AudioDeviceID, AudioObjectID,
    AudioObjectPropertyAddress,
};
use property_listener::AudioObjectPropertyListener;

pub use self::enumerate::{default_input_device, default_output_device, Devices};
use super::{asbd_from_config, check_os_status, host_time_to_stream_instant, OSStatus};
use crate::{
    host::{coreaudio::macos::loopback::LoopbackDevice, emit_error, try_emit_error},
    traits::{HostTrait, StreamTrait},
    Error, ErrorKind, FrameCount, ResultExt, StreamInstant,
};

mod device;
pub mod enumerate;
mod loopback;
mod property_listener;
pub use device::Device;

/// Coreaudio host, the default host on macOS.
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
type ErrorCallback = Box<dyn FnMut(Error) + Send + 'static>;

/// Spawns a dedicated thread that registers a single property listener and signals a channel on
/// each change. The listener is deregistered when the returned `Sender<()>` is dropped.
fn spawn_property_listener_thread(
    object_id: AudioObjectID,
    address: AudioObjectPropertyAddress,
) -> Result<(mpsc::Receiver<()>, mpsc::Sender<()>), Error> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let (change_tx, change_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel();

    std::thread::spawn(move || {
        let listener = AudioObjectPropertyListener::new(object_id, address, move || {
            let _ = change_tx.send(());
        });
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

    Ok((change_rx, shutdown_tx))
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

        // Spawn a dedicated thread to own both listeners. CoreAudio requires that
        // AudioObjectPropertyListeners are added and removed on the same thread.
        let disconnect_tx_alive = disconnect_tx.clone();
        let disconnect_tx_rate = disconnect_tx;
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
                        "device disconnected",
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
                        "device sample rate changed",
                    ));
                });

            match (alive_listener, rate_listener) {
                (Ok(_alive), Ok(_rate)) => {
                    let _ = ready_tx.send(Ok(()));
                    // Block until the stream is dropped; listeners are removed on drop.
                    let _ = shutdown_rx.recv();
                }
                (Err(e), _) | (_, Err(e)) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        });

        ready_rx.recv().map_err(|_| {
            Error::with_message(
                ErrorKind::StreamInvalidated,
                "disconnect listener thread terminated unexpectedly",
            )
        })??;

        std::thread::spawn(move || {
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
        });

        Ok(DisconnectManager {
            _shutdown_tx: shutdown_tx,
        })
    }
}

/// Manages the system default output device change listener on a dedicated thread.
///
/// When the system default output device changes:
/// - If a new valid default exists, AudioUnit reroutes and `DeviceChanged` is reported.
/// - If there is no new default, the stream is paused and `DeviceNotAvailable` is reported.
struct DefaultOutputMonitor {
    _shutdown_tx: mpsc::Sender<()>,
}

impl DefaultOutputMonitor {
    fn new(
        stream_weak: Weak<Mutex<StreamInner>>,
        error_callback: Arc<Mutex<ErrorCallback>>,
    ) -> Result<Self, Error> {
        let (change_rx, shutdown_tx) = spawn_property_listener_thread(
            kAudioObjectSystemObject as AudioObjectID,
            AudioObjectPropertyAddress {
                mSelector: kAudioHardwarePropertyDefaultOutputDevice,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            },
        )?;

        std::thread::spawn(move || {
            while let Ok(()) = change_rx.recv() {
                let Some(arc) = stream_weak.upgrade() else {
                    break;
                };
                if default_output_device().is_none() {
                    if let Ok(mut inner) = arc.try_lock() {
                        let _ = inner.pause();
                    }
                    emit_error(
                        &error_callback,
                        Error::with_message(
                            ErrorKind::DeviceNotAvailable,
                            "no default output device",
                        ),
                    );
                } else {
                    // DefaultOutput AudioUnit rerouted automatically; notify the caller.
                    try_emit_error(
                        &error_callback,
                        Error::with_message(
                            ErrorKind::DeviceChanged,
                            "default output device changed",
                        ),
                    );
                }
            }
        });

        Ok(DefaultOutputMonitor {
            _shutdown_tx: shutdown_tx,
        })
    }
}

struct StreamInner {
    playing: bool,
    audio_unit: AudioUnit,
    // Track the device with which the audio unit was spawned.
    //
    // We must do this so that we can avoid changing the device sample rate if there is already
    // a stream associated with the device.
    #[allow(dead_code)]
    device_id: AudioDeviceID,
    /// Manage the lifetime of the aggregate device used
    /// for loopback recording
    _loopback_device: Option<LoopbackDevice>,
}

impl StreamInner {
    fn play(&mut self) -> Result<(), Error> {
        if !self.playing {
            self.audio_unit
                .start()
                .context("failed to start audio unit")?;
            self.playing = true;
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Error> {
        if self.playing {
            self.audio_unit
                .stop()
                .context("failed to stop audio unit")?;
            self.playing = false;
        }
        Ok(())
    }
}

pub struct Stream {
    inner: Arc<Mutex<StreamInner>>,
    // Holds the device monitor (either DisconnectManager or DefaultOutputMonitor) to keep it
    // alive for the lifetime of the stream.
    _monitor: Box<dyn Send + Sync>,
}

impl Stream {
    fn new(inner: Arc<Mutex<StreamInner>>, monitor: Box<dyn Send + Sync>) -> Self {
        Self {
            inner,
            _monitor: monitor,
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        self.inner
            .lock()
            .map_err(|_| Error::with_message(ErrorKind::StreamInvalidated, "stream lock poisoned"))?
            .play()
    }

    fn pause(&self) -> Result<(), Error> {
        self.inner
            .lock()
            .map_err(|_| Error::with_message(ErrorKind::StreamInvalidated, "stream lock poisoned"))?
            .pause()
    }

    fn now(&self) -> StreamInstant {
        let m_host_time = unsafe { mach2::mach_time::mach_absolute_time() };
        host_time_to_stream_instant(m_host_time).expect("mach_timebase_info failed")
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        let stream = self.inner.lock().map_err(|_| {
            Error::with_message(ErrorKind::StreamInvalidated, "stream lock poisoned")
        })?;
        device::get_device_buffer_frame_size(&stream.audio_unit)
            .map(|size| size as FrameCount)
            .context("failed to get buffer frame size")
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
        println!("Device: {:?}", device.name());

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
