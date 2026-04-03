use std::time::Duration;

#[cfg(target_os = "emscripten")]
use wasm_bindgen::prelude::*;

/// A monotonic time instance associated with a stream, retrieved from either:
///
/// 1. A timestamp provided to the stream's underlying audio data callback or
/// 2. The same time source used to generate timestamps for a stream's underlying audio data
///    callback.
///
/// `StreamInstant` represents a moment on a stream's monotonic clock. Because the underlying clock
/// is monotonic, `StreamInstant` values are always positive and increasing.
///
/// Within a single stream, all instants share the same clock, so arithmetic between them is
/// meaningful. Across different streams, origins are not guaranteed to be shared. On some hosts
/// each stream starts its own independent clock at zero, so subtracting a timestamp from one
/// stream and one from another may produce a meaningless result.
///
/// ## Time sources by host
///
/// | Host | Time source |
/// | ---- | ----------- |
/// | AAudio | `AAudioStream_getTimestamp(CLOCK_MONOTONIC)` |
/// | ALSA | `snd_pcm_status_get_htstamp()` |
/// | ASIO | `timeGetTime()` |
/// | AudioWorklet | `AudioContext.currentTime` |
/// | CoreAudio | `mach_absolute_time()` |
/// | Emscripten | `AudioContext.currentTime` |
/// | JACK | `jack_get_time()` |
/// | PipeWire | `pw_stream_get_time_n()` |
/// | PulseAudio | `std::time::Instant` |
/// | WASAPI | `QueryPerformanceCounter()` |
/// | WebAudio | `AudioContext.currentTime` |
///
/// > **Disclaimer:** These system calls might change over time.
///
/// > **Note:** The `+` and `-` operators on `StreamInstant` may panic if the result cannot be
/// > represented as a `StreamInstant`. Use [`checked_add`][StreamInstant::checked_add] or
/// > [`checked_sub`][StreamInstant::checked_sub] for non-panicking variants.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct StreamInstant {
    secs: u64,
    nanos: u32,
}

/// A timestamp associated with a call to an input stream's data callback.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct InputStreamTimestamp {
    /// The instant the stream's data callback was invoked.
    pub callback: StreamInstant,
    /// The instant that data was captured from the device.
    ///
    /// E.g. The instant data was read from an ADC.
    pub capture: StreamInstant,
}

/// A timestamp associated with a call to an output stream's data callback.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct OutputStreamTimestamp {
    /// The instant the stream's data callback was invoked.
    pub callback: StreamInstant,
    /// The predicted instant that data written will be delivered to the device for playback.
    ///
    /// E.g. The instant data will be played by a DAC.
    pub playback: StreamInstant,
}

/// Information relevant to a single call to the user's input stream data callback.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct InputCallbackInfo {
    pub(crate) timestamp: InputStreamTimestamp,
}

/// Information relevant to a single call to the user's output stream data callback.
#[cfg_attr(target_os = "emscripten", wasm_bindgen)]
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct OutputCallbackInfo {
    pub(crate) timestamp: OutputStreamTimestamp,
}

impl StreamInstant {
    /// A `StreamInstant` with `secs` and `nanos` both set to zero.
    pub const ZERO: Self = Self { secs: 0, nanos: 0 };

    /// Returns the amount of time elapsed from `earlier` to `self`, or `None` if `earlier` is
    /// later than `self`.
    pub fn checked_duration_since(&self, earlier: StreamInstant) -> Option<Duration> {
        if self < &earlier {
            return None;
        }
        let delta = self.as_nanos() - earlier.as_nanos();
        let secs = u64::try_from(delta / 1_000_000_000).ok()?;
        let subsec_nanos = (delta % 1_000_000_000) as u32;
        Some(Duration::new(secs, subsec_nanos))
    }

    /// Returns the amount of time elapsed from `earlier` to `self`, saturating to
    /// [`Duration::ZERO`] if `earlier` is later than `self`.
    pub fn saturating_duration_since(&self, earlier: StreamInstant) -> Duration {
        self.checked_duration_since(earlier).unwrap_or_default()
    }

    /// Returns the amount of time elapsed from `earlier` to `self`, saturating to
    /// [`Duration::ZERO`] if `earlier` is later than `self`.
    pub fn duration_since(&self, earlier: StreamInstant) -> Duration {
        self.saturating_duration_since(earlier)
    }

    /// Returns `Some(t)` where `t` is `self + duration`, or `None` if the result cannot be
    /// represented as a `StreamInstant`.
    pub fn checked_add(&self, duration: Duration) -> Option<Self> {
        let total = self.as_nanos().checked_add(duration.as_nanos())?;
        let secs = u64::try_from(total / 1_000_000_000).ok()?;
        let nanos = (total % 1_000_000_000) as u32;
        Some(StreamInstant { secs, nanos })
    }

    /// Returns `Some(t)` where `t` is `self - duration`, or `None` if the result cannot be
    /// represented as a `StreamInstant` (i.e. would be negative).
    pub fn checked_sub(&self, duration: Duration) -> Option<Self> {
        let total = self.as_nanos().checked_sub(duration.as_nanos())?;
        let secs = u64::try_from(total / 1_000_000_000).ok()?;
        let nanos = (total % 1_000_000_000) as u32;
        Some(StreamInstant { secs, nanos })
    }

    /// Returns the total number of nanoseconds contained by this `StreamInstant`.
    pub fn as_nanos(&self) -> u128 {
        self.secs as u128 * 1_000_000_000 + self.nanos as u128
    }

    /// Creates a new `StreamInstant` from the specified number of nanoseconds.
    ///
    /// Note: Using this on the return value of `as_nanos()` might cause unexpected behavior:
    /// `as_nanos()` returns a `u128`, and can return values that do not fit in `u64`, e.g. 585
    /// years. Instead, consider using the pattern
    /// `StreamInstant::new(t.as_secs(), t.subsec_nanos())` if you cannot copy/clone the
    /// `StreamInstant` directly.
    pub fn from_nanos(nanos: u64) -> Self {
        let secs = nanos / 1_000_000_000;
        let subsec_nanos = (nanos % 1_000_000_000) as u32;
        Self::new(secs, subsec_nanos)
    }

    /// Creates a new `StreamInstant` from the specified number of milliseconds.
    pub fn from_millis(millis: u64) -> Self {
        Self::new(millis / 1_000, (millis % 1_000 * 1_000_000) as u32)
    }

    /// Creates a new `StreamInstant` from the specified number of microseconds.
    pub fn from_micros(micros: u64) -> Self {
        Self::new(micros / 1_000_000, (micros % 1_000_000 * 1_000) as u32)
    }

    /// Creates a new `StreamInstant` from the specified number of seconds represented as `f64`.
    ///
    /// # Panics
    ///
    /// Panics if `secs` is negative, not finite, or overflows the range of `StreamInstant`.
    pub fn from_secs_f64(secs: f64) -> Self {
        const NANOS_PER_SEC: u128 = 1_000_000_000;
        const MAX_NANOS: f64 = ((u64::MAX as u128 + 1) * NANOS_PER_SEC) as f64;
        let nanos = secs * NANOS_PER_SEC as f64;
        if !(0.0..MAX_NANOS).contains(&nanos) {
            panic!("StreamInstant::from_secs_f64 called with invalid value: {secs}");
        }
        let nanos = nanos as u128;
        Self::new(
            (nanos / NANOS_PER_SEC) as u64,
            (nanos % NANOS_PER_SEC) as u32,
        )
    }

    /// Creates a new `StreamInstant` from the specified number of whole seconds and additional
    /// nanoseconds.
    ///
    /// If `nanos` is greater than or equal to 1 billion (the number of nanoseconds in a second),
    /// the excess carries over into `secs`.
    ///
    /// # Panics
    ///
    /// Panics if the carry from `nanos` overflows the seconds counter.
    pub fn new(secs: u64, nanos: u32) -> Self {
        let carry = nanos / 1_000_000_000;
        let subsec_nanos = nanos % 1_000_000_000;
        let secs = secs
            .checked_add(carry as u64)
            .expect("overflow in StreamInstant::new");
        StreamInstant {
            secs,
            nanos: subsec_nanos,
        }
    }
}

impl std::ops::Add<Duration> for StreamInstant {
    type Output = StreamInstant;

    /// # Panics
    ///
    /// Panics if the result overflows the range of `StreamInstant`. Use
    /// [`checked_add`][StreamInstant::checked_add] for a non-panicking variant.
    #[inline]
    fn add(self, rhs: Duration) -> StreamInstant {
        self.checked_add(rhs)
            .expect("overflow when adding duration to stream instant")
    }
}

impl std::ops::AddAssign<Duration> for StreamInstant {
    #[inline]
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl std::ops::Sub<Duration> for StreamInstant {
    type Output = StreamInstant;

    /// # Panics
    ///
    /// Panics if the result underflows the range of `StreamInstant`. Use
    /// [`checked_sub`][StreamInstant::checked_sub] for a non-panicking variant.
    #[inline]
    fn sub(self, rhs: Duration) -> StreamInstant {
        self.checked_sub(rhs)
            .expect("overflow when subtracting duration from stream instant")
    }
}

impl std::ops::SubAssign<Duration> for StreamInstant {
    #[inline]
    fn sub_assign(&mut self, rhs: Duration) {
        *self = *self - rhs;
    }
}

impl std::ops::Sub<StreamInstant> for StreamInstant {
    type Output = Duration;

    /// Returns the duration from `rhs` to `self`, saturating to [`Duration::ZERO`] if `rhs` is
    /// later than `self`.
    #[inline]
    fn sub(self, rhs: StreamInstant) -> Duration {
        self.saturating_duration_since(rhs)
    }
}

impl InputCallbackInfo {
    pub fn new(timestamp: InputStreamTimestamp) -> Self {
        Self { timestamp }
    }

    /// The timestamp associated with the call to an input stream's data callback.
    pub fn timestamp(&self) -> InputStreamTimestamp {
        self.timestamp
    }
}

impl OutputCallbackInfo {
    pub fn new(timestamp: OutputStreamTimestamp) -> Self {
        Self { timestamp }
    }

    /// The timestamp associated with the call to an output stream's data callback.
    pub fn timestamp(&self) -> OutputStreamTimestamp {
        self.timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_instant() {
        let z = StreamInstant::ZERO; // origin
        let a = StreamInstant::new(2, 0);
        let max = StreamInstant::new(u64::MAX, 999_999_999); // largest representable instant

        assert_eq!(
            a.checked_sub(Duration::from_secs(1)),
            Some(StreamInstant::new(1, 0))
        );
        assert_eq!(
            a.checked_sub(Duration::from_secs(2)),
            Some(StreamInstant::ZERO)
        );
        assert_eq!(a.checked_sub(Duration::from_secs(3)), None); // would go below zero
        assert_eq!(z.checked_sub(Duration::from_nanos(1)), None); // underflow at origin

        assert_eq!(
            a.checked_add(Duration::from_secs(1)),
            Some(StreamInstant::new(3, 0))
        );
        assert_eq!(max.checked_add(Duration::from_nanos(1)), None); // overflow

        assert_eq!(a.duration_since(z), Duration::from_secs(2));
        assert_eq!(z.duration_since(a), Duration::ZERO); // saturates
        assert_eq!(a.checked_duration_since(z), Some(Duration::from_secs(2)));
        assert_eq!(z.checked_duration_since(a), None);
        assert_eq!(a.saturating_duration_since(z), Duration::from_secs(2));
        assert_eq!(z.saturating_duration_since(a), Duration::ZERO);

        assert_eq!(z + Duration::from_secs(2), a);
        assert_eq!(a - Duration::from_secs(2), z);
        assert_eq!(a - z, Duration::from_secs(2));
        assert_eq!(z - a, Duration::ZERO); // saturates via Sub<StreamInstant>
        let mut c = z;
        c += Duration::from_secs(2);
        assert_eq!(c, a);
        let mut d = a;
        d -= Duration::from_secs(2);
        assert_eq!(d, z);

        // nanosecond carry
        assert_eq!(
            StreamInstant::new(1, 1_500_000_000),
            StreamInstant::new(2, 500_000_000)
        );
        assert_eq!(
            StreamInstant::new(0, 1_000_000_000),
            StreamInstant::new(1, 0)
        );

        // basic round-trip
        assert_eq!(
            StreamInstant::from_secs_f64(1.5),
            StreamInstant::new(1, 500_000_000)
        );
        assert_eq!(StreamInstant::from_secs_f64(0.0), z);
    }

    #[test]
    #[should_panic]
    fn test_stream_instant_new_overflow() {
        StreamInstant::new(u64::MAX, 1_000_000_000); // carry overflows u64
    }

    #[test]
    #[should_panic]
    fn test_stream_instant_from_secs_f64_negative() {
        StreamInstant::from_secs_f64(-1.0);
    }

    #[test]
    #[should_panic]
    fn test_stream_instant_from_secs_f64_nan() {
        StreamInstant::from_secs_f64(f64::NAN);
    }

    #[test]
    #[should_panic]
    fn test_stream_instant_from_secs_f64_infinite() {
        StreamInstant::from_secs_f64(f64::INFINITY);
    }
}
