use std::convert::TryInto;
use std::time::Duration;

extern crate oboe;

use crate::{
    BackendSpecificError, BuildStreamError, PauseStreamError, PlayStreamError, StreamError,
    StreamInstant,
};

pub fn to_stream_instant(duration: Duration) -> StreamInstant {
    StreamInstant::new(
        duration.as_secs().try_into().unwrap(),
        duration.subsec_nanos(),
    )
}

pub fn stream_instant<T: oboe::AudioStreamSafe + ?Sized>(stream: &mut T) -> StreamInstant {
    const CLOCK_MONOTONIC: i32 = 1;
    let ts = stream
        .get_timestamp(CLOCK_MONOTONIC)
        .unwrap_or(oboe::FrameTimestamp {
            position: 0,
            timestamp: 0,
        });
    to_stream_instant(Duration::from_nanos(ts.timestamp as u64))
}

impl From<oboe::Error> for StreamError {
    fn from(error: oboe::Error) -> Self {
        use self::oboe::Error::*;
        match error {
            Disconnected | Unavailable | Closed => Self::DeviceNotAvailable,
            e => (BackendSpecificError {
                description: e.to_string(),
            })
            .into(),
        }
    }
}

impl From<oboe::Error> for PlayStreamError {
    fn from(error: oboe::Error) -> Self {
        use self::oboe::Error::*;
        match error {
            Disconnected | Unavailable | Closed => Self::DeviceNotAvailable,
            e => (BackendSpecificError {
                description: e.to_string(),
            })
            .into(),
        }
    }
}

impl From<oboe::Error> for PauseStreamError {
    fn from(error: oboe::Error) -> Self {
        use self::oboe::Error::*;
        match error {
            Disconnected | Unavailable | Closed => Self::DeviceNotAvailable,
            e => (BackendSpecificError {
                description: e.to_string(),
            })
            .into(),
        }
    }
}

impl From<oboe::Error> for BuildStreamError {
    fn from(error: oboe::Error) -> Self {
        use self::oboe::Error::*;
        match error {
            Disconnected | Unavailable | Closed => Self::DeviceNotAvailable,
            NoFreeHandles => Self::StreamIdOverflow,
            InvalidFormat | InvalidRate => Self::StreamConfigNotSupported,
            IllegalArgument => Self::InvalidArgument,
            e => (BackendSpecificError {
                description: e.to_string(),
            })
            .into(),
        }
    }
}
