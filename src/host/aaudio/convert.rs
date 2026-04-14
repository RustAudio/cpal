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

/// Projects a hardware timestamp anchor to the instant of a specific frame position.
fn stream_instant_from_anchor(
    anchor_frame: i64,
    anchor_nanos: i64,
    app_frame: i64,
    sample_rate: u32,
) -> StreamInstant {
    let offset_nanos =
        (app_frame as i128 - anchor_frame as i128) * 1_000_000_000 / sample_rate as i128;
    StreamInstant::from_nanos((anchor_nanos as i128 + offset_nanos).max(0) as u64)
}

/// Returns the [`StreamInstant`] for when the first frame of the current output callback will
/// be presented at the DAC.
pub fn output_stream_instant(stream: &ndk::audio::AudioStream, sample_rate: u32) -> StreamInstant {
    match stream.timestamp(ndk::audio::Clockid::Monotonic) {
        Ok(ts) => stream_instant_from_anchor(
            ts.frame_position,
            ts.time_nanoseconds,
            stream.frames_written(),
            sample_rate,
        ),
        Err(_) => now_stream_instant(),
    }
}

/// Returns the [`StreamInstant`] for when the first frame of the current input callback was
/// captured at the ADC.
pub fn input_stream_instant(stream: &ndk::audio::AudioStream, sample_rate: u32) -> StreamInstant {
    match stream.timestamp(ndk::audio::Clockid::Monotonic) {
        Ok(ts) => stream_instant_from_anchor(
            ts.frame_position,
            ts.time_nanoseconds,
            stream.frames_read(),
            sample_rate,
        ),
        Err(_) => now_stream_instant(),
    }
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
