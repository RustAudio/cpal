use crate::{ChannelCount, InputStreamTimestamp, OutputStreamTimestamp, SampleRate};

// Timing information for a duplex callback, combining input and output timestamps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DuplexCallbackInfo {
    input_timestamp: InputStreamTimestamp,
    output_timestamp: OutputStreamTimestamp,
}

impl DuplexCallbackInfo {
    pub fn new(
        input_timestamp: InputStreamTimestamp,
        output_timestamp: OutputStreamTimestamp,
    ) -> Self {
        Self {
            input_timestamp,
            output_timestamp,
        }
    }

    pub fn input_timestamp(&self) -> InputStreamTimestamp {
        self.input_timestamp
    }

    pub fn output_timestamp(&self) -> OutputStreamTimestamp {
        self.output_timestamp
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplexStreamConfig {
    pub input_channels: ChannelCount,
    pub output_channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub buffer_size: crate::BufferSize,
}
