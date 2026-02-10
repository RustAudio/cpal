#![allow(deprecated)]
use super::{asbd_from_config, check_os_status, frames_to_duration, host_time_to_stream_instant};

use super::OSStatus;
use crate::host::coreaudio::macos::loopback::LoopbackDevice;
use crate::traits::{HostTrait, StreamTrait};
use crate::{BackendSpecificError, DevicesError, PauseStreamError, PlayStreamError};
use coreaudio::audio_unit::AudioUnit;
use objc2_core_audio::AudioDeviceID;
use std::sync::{mpsc, Arc, Mutex, Weak};

pub use self::enumerate::{default_input_device, default_output_device, Devices};

use objc2_core_audio::{
    kAudioDevicePropertyDeviceIsAlive, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, AudioObjectPropertyAddress,
};
use property_listener::AudioObjectPropertyListener;

mod device;
pub mod enumerate;
mod loopback;
mod property_listener;
pub use device::Device;

/// Coreaudio host, the default host on macOS.
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
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

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
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
type ErrorCallback = Box<dyn FnMut(crate::StreamError) + Send + 'static>;

/// Invoke error callback, recovering from poisoned mutex if needed.
/// Returns true if callback was invoked, false if skipped due to WouldBlock.
#[inline]
fn invoke_error_callback<E>(error_callback: &Arc<Mutex<E>>, err: crate::StreamError) -> bool
where
    E: FnMut(crate::StreamError) + Send,
{
    match error_callback.try_lock() {
        Ok(mut cb) => {
            cb(err);
            true
        }
        Err(std::sync::TryLockError::Poisoned(guard)) => {
            // Recover from poisoned lock to still report this error
            guard.into_inner()(err);
            true
        }
        Err(std::sync::TryLockError::WouldBlock) => {
            // Skip if callback is busy
            false
        }
    }
}

/// Manages device disconnection listener on a dedicated thread to ensure the
/// AudioObjectPropertyListener is always created and dropped on the same thread.
/// This avoids potential threading issues with CoreAudio APIs.
///
/// When a device disconnects, this manager:
/// 1. Attempts to pause the stream to stop audio I/O
/// 2. Calls the error callback with `StreamError::DeviceNotAvailable`
///
/// The dedicated thread architecture ensures `Stream` can implement `Send`.
struct DisconnectManager {
    _shutdown_tx: mpsc::Sender<()>,
}

impl DisconnectManager {
    /// Create a new DisconnectManager that monitors device disconnection on a dedicated thread
    fn new(
        device_id: AudioDeviceID,
        stream_weak: Weak<Mutex<StreamInner>>,
        error_callback: Arc<Mutex<ErrorCallback>>,
    ) -> Result<Self, crate::BuildStreamError> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (disconnect_tx, disconnect_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        // Spawn dedicated thread to own the AudioObjectPropertyListener
        let disconnect_tx_clone = disconnect_tx.clone();
        std::thread::spawn(move || {
            let property_address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyDeviceIsAlive,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };

            // Create the listener on this dedicated thread
            let disconnect_fn = move || {
                let _ = disconnect_tx_clone.send(());
            };
            match AudioObjectPropertyListener::new(device_id, property_address, disconnect_fn) {
                Ok(_listener) => {
                    let _ = ready_tx.send(Ok(()));
                    // Drop the listener on this thread after receiving a shutdown signal
                    let _ = shutdown_rx.recv();
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        });

        // Wait for listener creation to complete or fail
        ready_rx
            .recv()
            .map_err(|_| crate::BuildStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "Disconnect listener thread terminated unexpectedly".to_string(),
                },
            })??;

        // Handle disconnect events on the main thread pool
        let stream_weak_clone = stream_weak.clone();
        let error_callback_clone = error_callback.clone();
        std::thread::spawn(move || {
            while disconnect_rx.recv().is_ok() {
                // Check if stream still exists
                if let Some(stream_arc) = stream_weak_clone.upgrade() {
                    // First, try to pause the stream to stop playback
                    if let Ok(mut stream_inner) = stream_arc.try_lock() {
                        let _ = stream_inner.pause();
                    }

                    // Always try to notify about device disconnection
                    invoke_error_callback(
                        &error_callback_clone,
                        crate::StreamError::DeviceNotAvailable,
                    );
                } else {
                    // Stream is gone, exit the handler thread
                    break;
                }
            }
        });

        Ok(DisconnectManager {
            _shutdown_tx: shutdown_tx,
        })
    }
}

/// Owned pointer to the duplex callback wrapper that is safe to send across threads.
///
/// SAFETY: The pointer is created via `Box::into_raw` on the build thread and shared with
/// CoreAudio via `inputProcRefCon`. CoreAudio dereferences it on every render callback on
/// its single-threaded audio thread for the lifetime of the stream. On drop, the audio unit
/// is stopped before reclaiming the `Box`, preventing use-after-free. `Send` is sound because
/// there is no concurrent mutable access—the build/drop thread never accesses the pointer
/// while the audio unit is running, and only reclaims it after stopping the audio unit.
struct DuplexCallbackPtr(*mut device::DuplexProcWrapper);

// SAFETY: See above — the pointer is shared with CoreAudio's audio thread but never
// accessed concurrently. The audio unit is stopped before reclaiming in drop.
unsafe impl Send for DuplexCallbackPtr {}

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
    /// Pointer to the duplex callback wrapper, manually managed for duplex streams.
    ///
    /// coreaudio-rs doesn't support duplex streams (enabling both input and output
    /// simultaneously), so we cannot use its `set_render_callback` API which would
    /// manage the callback lifetime automatically. Instead, we manually manage this
    /// callback pointer (created via `Box::into_raw`) and clean it up in Drop.
    ///
    /// This is None for regular input/output streams.
    duplex_callback_ptr: Option<DuplexCallbackPtr>,
}

impl StreamInner {
    fn play(&mut self) -> Result<(), PlayStreamError> {
        if !self.playing {
            if let Err(e) = self.audio_unit.start() {
                let description = e.to_string();
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            self.playing = true;
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), PauseStreamError> {
        if self.playing {
            if let Err(e) = self.audio_unit.stop() {
                let description = e.to_string();
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            self.playing = false;
        }
        Ok(())
    }
}

impl Drop for StreamInner {
    fn drop(&mut self) {
        // Clean up duplex callback if present.
        if let Some(DuplexCallbackPtr(ptr)) = self.duplex_callback_ptr {
            if !ptr.is_null() {
                // Stop the audio unit to ensure the callback is no longer being called
                // before reclaiming duplex_callback_ptr below. We must stop here regardless
                // of AudioUnit::drop's behavior.
                // Note: AudioUnit::drop will also call stop() — likely safe, but we stop here anyway.
                let _ = self.audio_unit.stop();
                // SAFETY: `ptr` was created via `Box::into_raw` in
                // `build_duplex_stream` and has not been reclaimed elsewhere.
                // The audio unit was stopped above, so the callback no longer
                // holds a reference to this pointer.
                unsafe {
                    let _ = Box::from_raw(ptr);
                }
            }
        }

        // AudioUnit's own Drop will handle uninitialize and dispose
        // _loopback_device's Drop will handle aggregate device cleanup
    }
}

pub struct Stream {
    inner: Arc<Mutex<StreamInner>>,
    // Manages the device disconnection listener separately to allow Stream to be Send.
    // The DisconnectManager contains the non-Send AudioObjectPropertyListener.
    _disconnect_manager: DisconnectManager,
}

impl Stream {
    fn new(
        inner: StreamInner,
        error_callback: ErrorCallback,
    ) -> Result<Self, crate::BuildStreamError> {
        let device_id = inner.device_id;
        let inner_arc = Arc::new(Mutex::new(inner));
        let weak_inner = Arc::downgrade(&inner_arc);

        let error_callback = Arc::new(Mutex::new(error_callback));
        let disconnect_manager = DisconnectManager::new(device_id, weak_inner, error_callback)?;

        Ok(Self {
            inner: inner_arc,
            _disconnect_manager: disconnect_manager,
        })
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let mut stream = self
            .inner
            .lock()
            .map_err(|_| PlayStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "A cpal stream operation panicked while holding the lock - this is a bug, please report it".to_string(),
                },
            })?;

        stream.play()
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        let mut stream = self
            .inner
            .lock()
            .map_err(|_| PauseStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "A cpal stream operation panicked while holding the lock - this is a bug, please report it".to_string(),
                },
            })?;

        stream.pause()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        default_host,
        traits::{DeviceTrait, HostTrait, StreamTrait},
        Sample,
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
                &config,
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
                &config,
                move |data: &[f32], _: &crate::InputCallbackInfo| {
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
                &config,
                move |data: &[f32], _: &crate::InputCallbackInfo| {
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

    fn write_silence<T: Sample>(data: &mut [T], _: &crate::OutputCallbackInfo) {
        for sample in data.iter_mut() {
            *sample = Sample::EQUILIBRIUM;
        }
    }
}
