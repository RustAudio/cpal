//! Time-conversion helpers for the AAudio backend.

extern crate ndk;

use crate::{
    BackendSpecificError, BuildStreamError, PauseStreamError, PlayStreamError, StreamError,
    StreamInstant,
};

/// Returns a [`StreamInstant`] for the current moment.
pub fn now_stream_instant() -> StreamInstant {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let res = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    assert_eq!(res, 0, "clock_gettime(CLOCK_MONOTONIC) failed");
    StreamInstant::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// Returns the [`StreamInstant`] of the most recent audio frame transferred by `stream`.
pub fn stream_instant(stream: &ndk::audio::AudioStream) -> StreamInstant {
    let ts = stream
        .timestamp(ndk::audio::Clockid::Monotonic)
        .unwrap_or(ndk::audio::Timestamp {
            frame_position: 0,
            time_nanoseconds: 0,
        });
    StreamInstant::from_nanos(ts.time_nanoseconds as u64)
}

impl From<ndk::audio::AudioError> for StreamError {
    fn from(error: ndk::audio::AudioError) -> Self {
        use self::ndk::audio::AudioError::*;
        match error {
            Disconnected | Unavailable => Self::DeviceNotAvailable,
            e => (BackendSpecificError {
                description: e.to_string(),
            })
            .into(),
        }
    }
}

impl From<ndk::audio::AudioError> for PlayStreamError {
    fn from(error: ndk::audio::AudioError) -> Self {
        use self::ndk::audio::AudioError::*;
        match error {
            Disconnected | Unavailable => Self::DeviceNotAvailable,
            e => (BackendSpecificError {
                description: e.to_string(),
            })
            .into(),
        }
    }
}

impl From<ndk::audio::AudioError> for PauseStreamError {
    fn from(error: ndk::audio::AudioError) -> Self {
        use self::ndk::audio::AudioError::*;
        match error {
            Disconnected | Unavailable => Self::DeviceNotAvailable,
            e => (BackendSpecificError {
                description: e.to_string(),
            })
            .into(),
        }
    }
}

impl From<ndk::audio::AudioError> for BuildStreamError {
    fn from(error: ndk::audio::AudioError) -> Self {
        use self::ndk::audio::AudioError::*;
        match error {
            Disconnected | Unavailable => Self::DeviceNotAvailable,
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
