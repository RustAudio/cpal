//! Duplex audio stream support with synchronized input/output.
//!
//! This module provides types for building duplex (simultaneous input/output) audio streams
//! with hardware clock synchronization.
//!
//! # Overview
//!
//! Unlike separate input and output streams which may have independent clocks, a duplex stream
//! uses a single device context for both input and output, ensuring they share the same
//! hardware clock. This is essential for applications like:
//!
//! - DAWs (Digital Audio Workstations)
//! - Real-time audio effects processing
//! - Audio measurement and analysis
//! - Any application requiring sample-accurate I/O synchronization
//!
//! # Example
//!
//! ```no_run
//! use cpal::duplex::DuplexStreamConfig;
//! use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//! use cpal::BufferSize;
//!
//! let host = cpal::default_host();
//! let device = host.default_output_device().expect("no device");
//!
//! let config = DuplexStreamConfig::symmetric(2, 48000, BufferSize::Fixed(512));
//!
//! let stream = device.build_duplex_stream::<f32, _, _>(
//!     &config,
//!     |input, output, info| {
//!         // Passthrough: copy input to output
//!         output[..input.len()].copy_from_slice(input);
//!     },
//!     |err| eprintln!("Stream error: {}", err),
//!     None,
//! ).expect("failed to build duplex stream");
//! ```

use crate::{SampleRate, StreamInstant};

/// Information passed to duplex callbacks.
///
/// This contains timing information for the current audio buffer, combining
/// both input and output timing similar to [`InputCallbackInfo`](crate::InputCallbackInfo)
/// and [`OutputCallbackInfo`](crate::OutputCallbackInfo).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DuplexCallbackInfo {
    /// The instant the stream's data callback was invoked.
    pub callback: StreamInstant,

    /// The instant that input data was captured from the device.
    ///
    /// This is calculated by subtracting the input device latency from the callback time,
    /// representing when the input samples were actually captured by the hardware (e.g., by an ADC).
    pub capture: StreamInstant,

    /// The predicted instant that output data will be delivered to the device for playback.
    ///
    /// This is calculated by adding the output device latency to the callback time,
    /// representing when the output samples will actually be played by the hardware (e.g., by a DAC).
    pub playback: StreamInstant,
}

impl DuplexCallbackInfo {
    /// Create a new DuplexCallbackInfo.
    pub fn new(callback: StreamInstant, capture: StreamInstant, playback: StreamInstant) -> Self {
        Self {
            callback,
            capture,
            playback,
        }
    }
}

/// Configuration for a duplex audio stream.
///
/// Unlike separate input/output streams, duplex streams require matching
/// configuration for both directions since they share a single device context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplexStreamConfig {
    /// Number of input channels.
    pub input_channels: u16,

    /// Number of output channels.
    pub output_channels: u16,

    /// Sample rate in Hz.
    pub sample_rate: SampleRate,

    /// Requested buffer size in frames.
    pub buffer_size: crate::BufferSize,
}

impl DuplexStreamConfig {
    /// Create a new duplex stream configuration.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - `input_channels` or `output_channels` is zero
    /// - `sample_rate` is zero
    /// - `buffer_size` is `BufferSize::Fixed(0)`
    pub fn new(
        input_channels: u16,
        output_channels: u16,
        sample_rate: SampleRate,
        buffer_size: crate::BufferSize,
    ) -> Self {
        assert!(input_channels > 0, "input_channels must be greater than 0");
        assert!(
            output_channels > 0,
            "output_channels must be greater than 0"
        );
        assert!(sample_rate > 0, "sample_rate must be greater than 0");
        assert!(
            !matches!(buffer_size, crate::BufferSize::Fixed(0)),
            "buffer_size cannot be Fixed(0)"
        );

        Self {
            input_channels,
            output_channels,
            sample_rate,
            buffer_size,
        }
    }

    /// Create a symmetric configuration (same channel count for input and output).
    ///
    /// # Panics
    ///
    /// Panics if `channels` is zero or if `sample_rate` is zero.
    pub fn symmetric(
        channels: u16,
        sample_rate: SampleRate,
        buffer_size: crate::BufferSize,
    ) -> Self {
        Self::new(channels, channels, sample_rate, buffer_size)
    }

    /// Convert to a basic StreamConfig using output channel count.
    ///
    /// Useful for compatibility with existing cpal APIs.
    pub fn to_stream_config(&self) -> crate::StreamConfig {
        crate::StreamConfig {
            channels: self.output_channels,
            sample_rate: self.sample_rate,
            buffer_size: self.buffer_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duplex_callback_info() {
        let callback = StreamInstant::new(1, 0);
        let capture = StreamInstant::new(0, 500_000_000); // 500ms before callback
        let playback = StreamInstant::new(1, 500_000_000); // 500ms after callback

        let info = DuplexCallbackInfo::new(callback, capture, playback);

        assert_eq!(info.callback, callback);
        assert_eq!(info.capture, capture);
        assert_eq!(info.playback, playback);
    }

    #[test]
    fn test_duplex_stream_config() {
        let config = DuplexStreamConfig::symmetric(2, 48000, crate::BufferSize::Fixed(512));
        assert_eq!(config.input_channels, 2);
        assert_eq!(config.output_channels, 2);
        assert_eq!(config.sample_rate, 48000);

        let stream_config = config.to_stream_config();
        assert_eq!(stream_config.channels, 2);
        assert_eq!(stream_config.sample_rate, 48000);
    }

    #[test]
    fn test_duplex_stream_config_asymmetric() {
        let config = DuplexStreamConfig::new(1, 8, 96000, crate::BufferSize::Default);
        assert_eq!(config.input_channels, 1);
        assert_eq!(config.output_channels, 8);
        assert_eq!(config.sample_rate, 96000);
    }

    #[test]
    fn test_duplex_stream_config_to_stream_config() {
        let config = DuplexStreamConfig::new(1, 2, 48000, crate::BufferSize::Fixed(256));
        let stream_config = config.to_stream_config();

        // to_stream_config uses output_channels
        assert_eq!(stream_config.channels, 2);
        assert_eq!(stream_config.sample_rate, 48000);
        assert_eq!(stream_config.buffer_size, crate::BufferSize::Fixed(256));
    }

    #[test]
    #[should_panic(expected = "input_channels must be greater than 0")]
    fn test_duplex_stream_config_zero_input_channels() {
        DuplexStreamConfig::new(0, 2, 48000, crate::BufferSize::Default);
    }

    #[test]
    #[should_panic(expected = "output_channels must be greater than 0")]
    fn test_duplex_stream_config_zero_output_channels() {
        DuplexStreamConfig::new(2, 0, 48000, crate::BufferSize::Default);
    }

    #[test]
    #[should_panic(expected = "sample_rate must be greater than 0")]
    fn test_duplex_stream_config_zero_sample_rate() {
        DuplexStreamConfig::new(2, 2, 0, crate::BufferSize::Default);
    }

    #[test]
    #[should_panic(expected = "buffer_size cannot be Fixed(0)")]
    fn test_duplex_stream_config_zero_buffer_size() {
        DuplexStreamConfig::new(2, 2, 48000, crate::BufferSize::Fixed(0));
    }

    #[test]
    fn test_duplex_stream_config_clone_and_eq() {
        let config1 = DuplexStreamConfig::new(2, 4, 48000, crate::BufferSize::Fixed(512));
        let config2 = config1.clone();

        assert_eq!(config1, config2);

        let config3 = DuplexStreamConfig::new(2, 4, 44100, crate::BufferSize::Fixed(512));
        assert_ne!(config1, config3);
    }
}
