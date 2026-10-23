use crate::{BufferSize, CallbackInfo, ChannelCount, SampleRate};

/// Information relevant to a single call to the user's duplex stream data callback.
///
/// `input.timestamp()` and `output.timestamp()` come from the same callback, so they are directly
/// comparable. Both directions should run on one synchronized clock, but have independent buffers,
/// so `input.xrun()` and `output.xrun()` can each report a glitch for the same invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DuplexCallbackInfo {
    pub(crate) input: CallbackInfo,
    pub(crate) output: CallbackInfo,
}

impl DuplexCallbackInfo {
    /// Construct a `DuplexCallbackInfo` from its input and output callback info.
    pub fn new(input: CallbackInfo, output: CallbackInfo) -> Self {
        Self { input, output }
    }

    /// The timestamp and xrun status for the captured input data passed to the callback.
    pub fn input(&self) -> CallbackInfo {
        self.input
    }

    /// The timestamp and xrun status for the output data written by the callback.
    pub fn output(&self) -> CallbackInfo {
        self.output
    }
}

/// The configuration shared by both directions of a duplex stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DuplexStreamConfig {
    /// The number of input (capture) channels.
    pub input_channels: ChannelCount,
    /// The number of output (playback) channels.
    pub output_channels: ChannelCount,
    /// The sample rate driving both directions.
    pub sample_rate: SampleRate,
    /// The desired buffer size, in frames per callback.
    pub buffer_size: BufferSize,
}

#[cfg(all(
    feature = "jack",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "macos",
        target_os = "windows",
    )
))]
/// Applies [`validate_config`] to each direction of a duplex configuration.
pub(crate) fn validate_duplex_config(config: &DuplexStreamConfig) -> Result<(), crate::Error> {
    let input = crate::StreamConfig {
        channels: config.input_channels,
        sample_rate: config.sample_rate,
        buffer_size: config.buffer_size,
    };
    crate::validate_stream_config(&input)?;

    let output = crate::StreamConfig {
        channels: config.output_channels,
        sample_rate: config.sample_rate,
        buffer_size: config.buffer_size,
    };
    crate::validate_stream_config(&output)
}
