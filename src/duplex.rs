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

use crate::{PauseStreamError, PlayStreamError, SampleRate, StreamInstant};

/// Hardware timestamp information from the audio device.
///
/// This provides precise timing information from the audio hardware, essential for
/// sample-accurate synchronization between input and output, and for correlating
/// audio timing with other system events.
///
/// # Detecting Xruns
///
/// Applications can detect xruns (buffer underruns/overruns) by tracking the
/// `sample_time` field across callbacks. Under normal operation, `sample_time`
/// advances by exactly the buffer size each callback. A larger jump indicates
/// missed buffers:
///
/// ```ignore
/// let mut last_sample_time: Option<f64> = None;
///
/// // In your callback:
/// if let Some(last) = last_sample_time {
///     let expected = last + buffer_size as f64;
///     let discontinuity = (info.timestamp.sample_time - expected).abs();
///     if discontinuity > 1.0 {
///         println!("Xrun detected: {} samples missed", discontinuity);
///     }
/// }
/// last_sample_time = Some(info.timestamp.sample_time);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioTimestamp {
    /// Hardware sample counter from the device clock.
    ///
    /// This is the authoritative position from the device's clock and increments
    /// by the buffer size each callback. Use this for xrun detection by tracking
    /// discontinuities.
    ///
    /// This is an f64 to allow for sub-sample precision in rate-adjusted scenarios.
    /// For most purposes, cast to i64 for an integer value.
    pub sample_time: f64,

    /// System host time reference (platform-specific high-resolution timer).
    ///
    /// Can be used to correlate audio timing with other system events or for
    /// debugging latency issues.
    pub host_time: u64,

    /// Clock rate scalar (1.0 = nominal rate).
    ///
    /// Indicates if the hardware clock is running faster or slower than nominal.
    /// Useful for applications that need to compensate for clock drift when
    /// synchronizing with external sources.
    pub rate_scalar: f64,

    /// Callback timestamp from cpal's existing timing system.
    ///
    /// This provides compatibility with cpal's existing `StreamInstant` timing
    /// infrastructure.
    pub callback_instant: StreamInstant,
}

impl AudioTimestamp {
    /// Create a new AudioTimestamp.
    pub fn new(
        sample_time: f64,
        host_time: u64,
        rate_scalar: f64,
        callback_instant: StreamInstant,
    ) -> Self {
        Self {
            sample_time,
            host_time,
            rate_scalar,
            callback_instant,
        }
    }

    /// Get the sample position as an integer.
    ///
    /// This rounds the hardware sample time to the nearest integer. The result
    /// is suitable for use as a timeline position or for sample-accurate event
    /// scheduling.
    #[inline]
    pub fn sample_position(&self) -> i64 {
        self.sample_time.round() as i64
    }

    /// Check if the clock is running at nominal rate.
    ///
    /// Returns `true` if `rate_scalar` is very close to 1.0 (within 0.0001).
    #[inline]
    pub fn is_nominal_rate(&self) -> bool {
        (self.rate_scalar - 1.0).abs() < 0.0001
    }
}

impl Default for AudioTimestamp {
    fn default() -> Self {
        Self {
            sample_time: 0.0,
            host_time: 0,
            rate_scalar: 1.0,
            callback_instant: StreamInstant::new(0, 0),
        }
    }
}

/// Information passed to duplex callbacks.
///
/// This contains timing information and metadata about the current audio buffer.
#[derive(Clone, Copy, Debug)]
pub struct DuplexCallbackInfo {
    /// Hardware timestamp for this callback.
    pub timestamp: AudioTimestamp,
}

impl DuplexCallbackInfo {
    /// Create a new DuplexCallbackInfo.
    pub fn new(timestamp: AudioTimestamp) -> Self {
        Self { timestamp }
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

/// A placeholder duplex stream type for backends that don't yet support duplex.
///
/// This type implements `StreamTrait` but all operations return errors.
/// Backend implementations should replace this with their own type once
/// duplex support is implemented.
pub struct UnsupportedDuplexStream {
    _private: (),
}

impl UnsupportedDuplexStream {
    /// Create a new unsupported duplex stream marker.
    ///
    /// This should not normally be called - it exists only to satisfy
    /// type requirements for backends without duplex support.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for UnsupportedDuplexStream {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::traits::StreamTrait for UnsupportedDuplexStream {
    fn play(&self) -> Result<(), PlayStreamError> {
        Err(PlayStreamError::BackendSpecific {
            err: crate::BackendSpecificError {
                description: "Duplex streams are not yet supported on this backend".to_string(),
            },
        })
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        Err(PauseStreamError::BackendSpecific {
            err: crate::BackendSpecificError {
                description: "Duplex streams are not yet supported on this backend".to_string(),
            },
        })
    }
}

// Safety: UnsupportedDuplexStream contains no mutable state
unsafe impl Send for UnsupportedDuplexStream {}
unsafe impl Sync for UnsupportedDuplexStream {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_timestamp_sample_position() {
        let ts = AudioTimestamp::new(1234.5, 0, 1.0, StreamInstant::new(0, 0));
        assert_eq!(ts.sample_position(), 1235); // rounds up

        let ts = AudioTimestamp::new(1234.4, 0, 1.0, StreamInstant::new(0, 0));
        assert_eq!(ts.sample_position(), 1234); // rounds down

        let ts = AudioTimestamp::new(-100.0, 0, 1.0, StreamInstant::new(0, 0));
        assert_eq!(ts.sample_position(), -100); // negative values work
    }

    #[test]
    fn test_audio_timestamp_nominal_rate() {
        let ts = AudioTimestamp::new(0.0, 0, 1.0, StreamInstant::new(0, 0));
        assert!(ts.is_nominal_rate());

        let ts = AudioTimestamp::new(0.0, 0, 1.00005, StreamInstant::new(0, 0));
        assert!(ts.is_nominal_rate()); // within tolerance

        let ts = AudioTimestamp::new(0.0, 0, 1.001, StreamInstant::new(0, 0));
        assert!(!ts.is_nominal_rate()); // outside tolerance
    }

    #[test]
    fn test_audio_timestamp_default() {
        let ts = AudioTimestamp::default();
        assert_eq!(ts.sample_time, 0.0);
        assert_eq!(ts.host_time, 0);
        assert_eq!(ts.rate_scalar, 1.0);
        assert_eq!(ts.sample_position(), 0);
        assert!(ts.is_nominal_rate());
    }

    #[test]
    fn test_audio_timestamp_equality() {
        let ts1 = AudioTimestamp::new(1000.0, 12345, 1.0, StreamInstant::new(0, 0));
        let ts2 = AudioTimestamp::new(1000.0, 12345, 1.0, StreamInstant::new(0, 0));
        let ts3 = AudioTimestamp::new(1000.0, 12346, 1.0, StreamInstant::new(0, 0));

        assert_eq!(ts1, ts2);
        assert_ne!(ts1, ts3);
    }

    #[test]
    fn test_duplex_callback_info() {
        let ts = AudioTimestamp::new(512.0, 1000, 1.0, StreamInstant::new(0, 0));
        let info = DuplexCallbackInfo::new(ts);
        assert_eq!(info.timestamp.sample_time, 512.0);
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

    #[test]
    fn test_unsupported_duplex_stream() {
        use crate::traits::StreamTrait;

        let stream = UnsupportedDuplexStream::new();

        // play() should return an error
        let play_result = stream.play();
        assert!(play_result.is_err());

        // pause() should return an error
        let pause_result = stream.pause();
        assert!(pause_result.is_err());
    }

    #[test]
    fn test_unsupported_duplex_stream_default() {
        let _stream = UnsupportedDuplexStream::default();
    }

    #[test]
    fn test_unsupported_duplex_stream_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UnsupportedDuplexStream>();
    }
}
