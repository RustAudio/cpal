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
//! let config = DuplexStreamConfig {
//!     input_channels: 2,
//!     output_channels: 2,
//!     sample_rate: 48000,
//!     buffer_size: BufferSize::Fixed(512),
//! };
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
