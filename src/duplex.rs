use crate::{BufferSize, CallbackInfo, ChannelCount, SampleRate};

/// Information relevant to a single call to the user's duplex stream data callback.
///
/// Because a duplex stream's input and output share a single clock, `input.timestamp()` and
/// `output.timestamp()` are drawn from the same time source. The two directions have independent
/// buffers, so `input.xrun()` and `output.xrun()` can each report a glitch independently for the
/// same invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DuplexCallbackInfo {
    input: CallbackInfo,
    output: CallbackInfo,
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
