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

// ============================================================================
// DuplexStream - Synchronized input/output with shared hardware clock
// ============================================================================

/// Internal state for the duplex stream.
pub(crate) struct DuplexStreamInner {
    pub(crate) playing: bool,
    pub(crate) audio_unit: AudioUnit,
    pub(crate) device_id: AudioDeviceID,
    /// Pointer to the callback wrapper, needed for cleanup.
    /// This is set by build_duplex_stream_raw and freed in Drop.
    pub(crate) duplex_callback_ptr: *mut device::DuplexProcWrapper,
}

// SAFETY: DuplexStreamInner is Send because:
// 1. AudioUnit is Send (coreaudio crate marks it as such)
// 2. AudioDeviceID is Copy
// 3. duplex_callback_ptr points to a Send type (DuplexProcWrapper)
//    and is only accessed during Drop after stopping the audio unit
unsafe impl Send for DuplexStreamInner {}

impl DuplexStreamInner {
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

impl Drop for DuplexStreamInner {
    fn drop(&mut self) {
        // Stop the audio unit first to ensure callback is no longer being called
        let _ = self.audio_unit.stop();

        // Now safe to free the callback wrapper
        if !self.duplex_callback_ptr.is_null() {
            unsafe {
                let _ = Box::from_raw(self.duplex_callback_ptr);
            }
            self.duplex_callback_ptr = std::ptr::null_mut();
        }

        // AudioUnit's own Drop will handle uninitialize and dispose
    }
}

/// Duplex stream disconnect manager - handles device disconnection.
struct DuplexDisconnectManager {
    _shutdown_tx: mpsc::Sender<()>,
}

impl DuplexDisconnectManager {
    fn new(
        device_id: AudioDeviceID,
        stream_weak: Weak<Mutex<DuplexStreamInner>>,
        error_callback: Arc<Mutex<ErrorCallback>>,
    ) -> Result<Self, crate::BuildStreamError> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (disconnect_tx, disconnect_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        let disconnect_tx_clone = disconnect_tx.clone();
        std::thread::spawn(move || {
            let property_address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyDeviceIsAlive,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            };

            match AudioObjectPropertyListener::new(device_id, property_address, move || {
                let _ = disconnect_tx_clone.send(());
            }) {
                Ok(_listener) => {
                    let _ = ready_tx.send(Ok(()));
                    let _ = shutdown_rx.recv();
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        });

        ready_rx
            .recv()
            .map_err(|_| crate::BuildStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "Disconnect listener thread terminated unexpectedly".to_string(),
                },
            })??;

        // Handle disconnect events
        std::thread::spawn(move || {
            while disconnect_rx.recv().is_ok() {
                if let Some(stream_arc) = stream_weak.upgrade() {
                    if let Ok(mut stream_inner) = stream_arc.try_lock() {
                        let _ = stream_inner.pause();
                    }
                    invoke_error_callback(&error_callback, crate::StreamError::DeviceNotAvailable);
                } else {
                    break;
                }
            }
        });

        Ok(DuplexDisconnectManager {
            _shutdown_tx: shutdown_tx,
        })
    }
}

/// A duplex audio stream with synchronized input and output.
///
/// Uses a single HAL AudioUnit with both input and output enabled,
/// ensuring they share the same hardware clock.
pub struct DuplexStream {
    inner: Arc<Mutex<DuplexStreamInner>>,
    _disconnect_manager: DuplexDisconnectManager,
}

// Compile-time assertion that DuplexStream is Send and Sync
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DuplexStream>();
};

impl DuplexStream {
    /// Create a new duplex stream.
    ///
    /// This is called by `Device::build_duplex_stream_raw`.
    pub(crate) fn new(
        inner: DuplexStreamInner,
        error_callback: ErrorCallback,
    ) -> Result<Self, crate::BuildStreamError> {
        let device_id = inner.device_id;
        let inner_arc = Arc::new(Mutex::new(inner));
        let weak_inner = Arc::downgrade(&inner_arc);

        let error_callback = Arc::new(Mutex::new(error_callback));

        let disconnect_manager =
            DuplexDisconnectManager::new(device_id, weak_inner, error_callback)?;

        Ok(Self {
            inner: inner_arc,
            _disconnect_manager: disconnect_manager,
        })
    }
}

impl StreamTrait for DuplexStream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let mut stream = self
            .inner
            .lock()
            .map_err(|_| PlayStreamError::BackendSpecific {
                err: BackendSpecificError {
                    description: "A cpal duplex stream operation panicked while holding the lock"
                        .to_string(),
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
                    description: "A cpal duplex stream operation panicked while holding the lock"
                        .to_string(),
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

    #[test]
    fn test_duplex_stream() {
        use crate::duplex::DuplexStreamConfig;
        use crate::BufferSize;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        // Skip in CI due to audio device permissions
        if std::env::var("CI").is_ok() {
            println!("Skipping test_duplex_stream in CI environment due to permissions");
            return;
        }

        let host = default_host();
        let device = host.default_output_device().expect("no output device");

        // Check if device supports both input and output
        let has_input = device
            .supported_input_configs()
            .map(|mut configs| configs.next().is_some())
            .unwrap_or(false);
        let has_output = device
            .supported_output_configs()
            .map(|mut configs| configs.next().is_some())
            .unwrap_or(false);

        if !has_input || !has_output {
            println!("Skipping test_duplex_stream: device doesn't support both input and output");
            return;
        }

        let callback_count = Arc::new(AtomicU32::new(0));
        let callback_count_clone = callback_count.clone();

        // Get supported sample rates from output config
        let output_config = device
            .supported_output_configs()
            .unwrap()
            .next()
            .unwrap()
            .with_max_sample_rate();

        let config = DuplexStreamConfig {
            input_channels: 2,
            output_channels: 2,
            sample_rate: output_config.sample_rate(),
            buffer_size: BufferSize::Default,
        };

        println!("Building duplex stream with config: {:?}", config);

        let stream = device.build_duplex_stream::<f32, _, _>(
            &config,
            move |input, output, _info| {
                callback_count_clone.fetch_add(1, Ordering::Relaxed);
                // Simple passthrough: copy input to output
                let copy_len = input.len().min(output.len());
                output[..copy_len].copy_from_slice(&input[..copy_len]);
                // Zero any remaining output
                for sample in output[copy_len..].iter_mut() {
                    *sample = 0.0;
                }
            },
            |err| println!("Error: {err}"),
            None,
        );

        match stream {
            Ok(stream) => {
                stream.play().unwrap();
                std::thread::sleep(std::time::Duration::from_millis(500));
                stream.pause().unwrap();

                let count = callback_count.load(Ordering::Relaxed);
                println!("Duplex callback was called {} times", count);
                assert!(
                    count > 0,
                    "Duplex callback should have been called at least once"
                );
            }
            Err(e) => {
                // This is acceptable if the device doesn't truly support duplex
                println!("Could not create duplex stream: {:?}", e);
            }
        }
    }

    /// Test that verifies duplex synchronization by checking timestamp continuity.
    #[test]
    fn test_duplex_synchronization_verification() {
        use crate::duplex::DuplexStreamConfig;
        use crate::BufferSize;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::{Arc, Mutex};

        // Skip in CI due to audio device permissions
        if std::env::var("CI").is_ok() {
            println!("Skipping duplex sync test in CI environment");
            return;
        }

        let host = default_host();
        let device = host.default_output_device().expect("no output device");

        // Check device capabilities
        let has_input = device
            .supported_input_configs()
            .map(|mut c| c.next().is_some())
            .unwrap_or(false);
        let has_output = device
            .supported_output_configs()
            .map(|mut c| c.next().is_some())
            .unwrap_or(false);

        if !has_input || !has_output {
            println!("Skipping: device doesn't support both input and output");
            return;
        }

        /// Verification state collected during callbacks
        #[derive(Debug, Default)]
        struct SyncVerificationState {
            callback_count: u64,
            total_frames: u64,
            last_sample_time: Option<f64>,
            discontinuity_count: u64,
            timestamp_regressions: u64,
        }

        let state = Arc::new(Mutex::new(SyncVerificationState::default()));
        let state_clone = state.clone();

        // Get device config
        let output_config = device
            .supported_output_configs()
            .unwrap()
            .next()
            .unwrap()
            .with_max_sample_rate();

        let sample_rate = output_config.sample_rate();
        let input_channels = 2u16;
        let output_channels = 2u16;
        let buffer_size = 512u32;

        let config = DuplexStreamConfig {
            input_channels,
            output_channels,
            sample_rate,
            buffer_size: BufferSize::Fixed(buffer_size),
        };

        println!("=== Duplex Synchronization Verification Test ===");
        println!("Config: {:?}", config);

        let error_count = Arc::new(AtomicU64::new(0));
        let error_count_cb = error_count.clone();

        let stream = match device.build_duplex_stream::<f32, _, _>(
            &config,
            move |input, output, info| {
                let mut state = state_clone.lock().unwrap();
                state.callback_count += 1;

                // Calculate frames from output buffer size
                let frames = output.len() / output_channels as usize;
                state.total_frames += frames as u64;

                // Check for timestamp discontinuities
                if let Some(prev_sample_time) = state.last_sample_time {
                    let expected = prev_sample_time + frames as f64;
                    let discontinuity = (info.timestamp.sample_time - expected).abs();

                    if discontinuity > 1.0 {
                        state.discontinuity_count += 1;
                    }

                    if info.timestamp.sample_time < prev_sample_time {
                        state.timestamp_regressions += 1;
                    }
                }

                state.last_sample_time = Some(info.timestamp.sample_time);

                // Simple passthrough
                let copy_len = input.len().min(output.len());
                output[..copy_len].copy_from_slice(&input[..copy_len]);
                for sample in output[copy_len..].iter_mut() {
                    *sample = 0.0;
                }
            },
            move |err| {
                println!("Stream error: {err}");
                error_count_cb.fetch_add(1, Ordering::Relaxed);
            },
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                println!("Could not create duplex stream: {:?}", e);
                return;
            }
        };

        // Run for 1 second
        println!("Running duplex stream for 1 second...");
        stream.play().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        stream.pause().unwrap();

        // Collect results
        let state = state.lock().unwrap();
        let stream_errors = error_count.load(Ordering::Relaxed);

        println!("\n=== Verification Results ===");
        println!("Callbacks: {}", state.callback_count);
        println!("Total frames: {}", state.total_frames);
        println!("Discontinuities: {}", state.discontinuity_count);
        println!("Timestamp regressions: {}", state.timestamp_regressions);
        println!("Stream errors: {}", stream_errors);

        // Assertions
        assert!(
            state.callback_count > 0,
            "Callback should have been called at least once"
        );
        assert_eq!(
            state.timestamp_regressions, 0,
            "Timestamps should never regress"
        );
        assert_eq!(stream_errors, 0, "No stream errors should occur");
        assert!(
            state.discontinuity_count <= 5,
            "Too many discontinuities: {} (max allowed: 5)",
            state.discontinuity_count
        );

        println!("\n=== All synchronization checks PASSED ===");
    }
}
