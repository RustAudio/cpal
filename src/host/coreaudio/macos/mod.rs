#![allow(deprecated)]
use super::{asbd_from_config, check_os_status, frames_to_duration, host_time_to_stream_instant};

use super::OSStatus;
use crate::host::coreaudio::macos::loopback::LoopbackDevice;
use crate::traits::{HostTrait, StreamTrait};
use crate::{BackendSpecificError, DevicesError, PauseStreamError, PlayStreamError};
use coreaudio::audio_unit::AudioUnit;
use objc2_core_audio::AudioDeviceID;
use std::cell::RefCell;
use std::rc::Rc;

pub use self::enumerate::{default_input_device, default_output_device, Devices};

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

struct StreamInner {
    playing: bool,
    audio_unit: AudioUnit,
    /// Manage the lifetime of the closure that handles device disconnection.
    _disconnect_listener: Option<AudioObjectPropertyListener>,
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
                let description = format!("{e}");
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
                let description = format!("{e}");
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            self.playing = false;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Stream {
    inner: Rc<RefCell<StreamInner>>,
}

impl Stream {
    fn new(inner: StreamInner) -> Self {
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let mut stream = self.inner.borrow_mut();

        stream.play()
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        let mut stream = self.inner.borrow_mut();

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
    #[cfg(target_os = "macos")]
    fn test_buffer_size_equivalence() {
        use crate::{BufferSize, SampleRate, StreamConfig};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let host = default_host();
        let device = host.default_output_device().unwrap();

        // Step 1: Open stream with BufferSize::Default and measure callback size
        let default_config = StreamConfig {
            channels: 2,
            sample_rate: SampleRate(48000),
            buffer_size: BufferSize::Default,
        };

        let default_buffer_sizes = Arc::new(Mutex::new(Vec::new()));
        let default_buffer_sizes_clone = default_buffer_sizes.clone();

        let default_stream = device
            .build_output_stream(
                &default_config,
                move |data: &mut [f32], info: &crate::OutputCallbackInfo| {
                    let mut sizes = default_buffer_sizes_clone.lock().unwrap();
                    sizes.push(data.len());
                    write_silence(data, info);
                },
                move |err| println!("Error: {err}"),
                None,
            )
            .unwrap();

        default_stream.play().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        default_stream.pause().unwrap();

        let default_sizes = default_buffer_sizes.lock().unwrap().clone();
        assert!(
            !default_sizes.is_empty(),
            "Should have captured some buffer sizes"
        );

        let x = default_sizes[0]; // This is the callback size we got with Default
        println!("Default stream callback size: {} samples", x);

        // Step 2: Open stream with BufferSize::Fixed(x) and verify we get callback size x again
        let fixed_config = StreamConfig {
            channels: 2,
            sample_rate: SampleRate(48000),
            buffer_size: BufferSize::Fixed(x as u32),
        };

        let fixed_buffer_sizes = Arc::new(Mutex::new(Vec::new()));
        let fixed_buffer_sizes_clone = fixed_buffer_sizes.clone();

        let fixed_stream = device
            .build_output_stream(
                &fixed_config,
                move |data: &mut [f32], info: &crate::OutputCallbackInfo| {
                    let mut sizes = fixed_buffer_sizes_clone.lock().unwrap();
                    sizes.push(data.len());
                    write_silence(data, info);
                },
                move |err| println!("Error: {err}"),
                None,
            )
            .unwrap();

        fixed_stream.play().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        fixed_stream.pause().unwrap();

        let fixed_sizes = fixed_buffer_sizes.lock().unwrap().clone();
        assert!(
            !fixed_sizes.is_empty(),
            "Should have captured some buffer sizes"
        );

        let fixed_callback_size = fixed_sizes[0];
        println!(
            "Fixed stream callback size: {} samples",
            fixed_callback_size
        );

        // Step 3: Verify that BufferSize::Fixed(x) gives us callback size x
        assert_eq!(
            x, fixed_callback_size,
            "BufferSize::Fixed({}) should produce callback size {}, but got {}",
            x, x, fixed_callback_size
        );

        // Also verify consistency within each stream
        for &size in default_sizes.iter() {
            assert_eq!(size, x, "Default stream had inconsistent callback sizes");
        }
        for &size in fixed_sizes.iter() {
            assert_eq!(size, x, "Fixed stream had inconsistent callback sizes");
        }

        println!("Buffer size equivalence test passed: Default and Fixed({}) both produce callback size {}", x, x);
    }
}
