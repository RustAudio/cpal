use crate::{ChannelCount, InputStreamTimestamp, OutputStreamTimestamp, SampleRate};

/// Information relevant to a single call to the user's duplex stream data callback.
///
/// Combines the input and output timestamps for the callback. Because a duplex stream's input and
/// output share a single clock, both timestamps are drawn from the same time source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DuplexCallbackInfo {
    input_timestamp: InputStreamTimestamp,
    output_timestamp: OutputStreamTimestamp,
}

impl DuplexCallbackInfo {
    /// Construct a `DuplexCallbackInfo` from its input and output timestamps.
    pub fn new(
        input_timestamp: InputStreamTimestamp,
        output_timestamp: OutputStreamTimestamp,
    ) -> Self {
        Self {
            input_timestamp,
            output_timestamp,
        }
    }

    /// The timestamp for the captured input data passed to the callback.
    pub fn input_timestamp(&self) -> InputStreamTimestamp {
        self.input_timestamp
    }

    /// The timestamp for the output data written by the callback.
    pub fn output_timestamp(&self) -> OutputStreamTimestamp {
        self.output_timestamp
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
    pub buffer_size: crate::BufferSize,
}
