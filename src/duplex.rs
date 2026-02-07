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

use crate::{InputStreamTimestamp, OutputStreamTimestamp, SampleRate};

/// Information passed to duplex callbacks.
///
/// This contains timing information for the current audio buffer, combining
/// both input and output timing. A duplex stream has a single callback invocation
/// that provides synchronized input and output data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DuplexCallbackInfo {
    input_timestamp: InputStreamTimestamp,
    output_timestamp: OutputStreamTimestamp,
}

impl DuplexCallbackInfo {
    /// Create a new DuplexCallbackInfo.
    ///
    /// Note: Both timestamps will share the same `callback` instant since there is
    /// only one callback invocation for a duplex stream.
    pub fn new(
        input_timestamp: InputStreamTimestamp,
        output_timestamp: OutputStreamTimestamp,
    ) -> Self {
        Self {
            input_timestamp,
            output_timestamp,
        }
    }

    /// The timestamp for the input portion of the duplex stream.
    ///
    /// Contains the callback instant and when the input data was captured.
    pub fn input_timestamp(&self) -> InputStreamTimestamp {
        self.input_timestamp
    }

    /// The timestamp for the output portion of the duplex stream.
    ///
    /// Contains the callback instant and when the output data will be played.
    pub fn output_timestamp(&self) -> OutputStreamTimestamp {
        self.output_timestamp
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
