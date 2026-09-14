use super::{alsa, stream::TimestampMode};
use crate::{Error, StreamInstant};

#[inline]
pub(super) fn callback_instant_for(
    timestamp_mode: TimestampMode,
    creation_ts: alsa::timespec,
    creation_instant: std::time::Instant,
    status: &alsa::pcm::Status,
) -> StreamInstant {
    // For playback the PCM starts in PREPARED state while the output buffer fills;
    // snd_pcm_start() fires automatically at start_threshold, moving it to RUNNING.
    // Therefore, callbacks arrive before RUNNING state. Using creation_ts as the
    // anchor for all modes means timestamps advance monotonically through both the
    // initial buffer fill and any later xrun recovery.
    match timestamp_mode {
        TimestampMode::CreationInstant => {
            let d = std::time::Instant::now().duration_since(creation_instant);
            StreamInstant::new(d.as_secs(), d.subsec_nanos())
        }
        TimestampMode::SystemClock => {
            // htstamp is the time of the most recent DMA interrupt on the configured
            // monotonic clock. Subtracting creation_ts (same clock, prepare() time)
            // gives elapsed time since stream creation in any PCM state.
            htstamp_elapsed(status, creation_ts)
        }
        TimestampMode::AudioLink => {
            // audio_htstamp measures elapsed time since snd_pcm_start() via hardware
            // sample counter and TSC cross-timestamp, so it is only valid in RUNNING state.
            if status.get_state() != alsa::pcm::State::Running {
                // After xrun recovery, snd_pcm_prepare() does not reset trigger_htstamp
                // (only snd_pcm_start() does), so it keeps its pre-xrun value while the
                // hardware counter has not yet restarted.
                htstamp_elapsed(status, creation_ts)
            } else {
                // When running, add (trigger_ts - creation_ts) to express elapsed time
                // since stream creation rather than since the last snd_pcm_start().
                let trigger_ts = status.get_trigger_htstamp();
                let trigger_offset = timespec_diff_nanos(trigger_ts, creation_ts);
                if trigger_offset < 0 {
                    // trigger_ts predates creation_ts (driver bug); fall back to
                    // htstamp - creation_ts to preserve a monotone result.
                    htstamp_elapsed(status, creation_ts)
                } else {
                    let audio_ts = status.get_audio_htstamp();
                    let nanos = timespec_to_nanos(audio_ts) + trigger_offset;
                    StreamInstant::from_nanos(nanos as u64)
                }
            }
        }
    }
}

#[inline]
pub(super) fn status_with_timestamp(
    handle: &alsa::pcm::PCM,
    mode: TimestampMode,
) -> Result<alsa::pcm::Status, Error> {
    let audio_ts_type = match mode {
        TimestampMode::AudioLink => alsa::pcm::AudioTstampType::LinkSynchronized,
        TimestampMode::SystemClock | TimestampMode::CreationInstant => {
            alsa::pcm::AudioTstampType::Compat
        }
    };
    alsa::pcm::StatusBuilder::new()
        .audio_htstamp_config(audio_ts_type, false)
        .build(handle)
        .map_err(Into::into)
}

// Adapted from `timestamp2ns` here:
// https://fossies.org/linux/alsa-lib/test/audio_time.c
#[inline]
#[expect(clippy::unnecessary_cast)]
fn timespec_to_nanos(ts: alsa::timespec) -> i64 {
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

// Adapted from `timediff` here:
// https://fossies.org/linux/alsa-lib/test/audio_time.c
#[inline]
fn timespec_diff_nanos(a: alsa::timespec, b: alsa::timespec) -> i64 {
    timespec_to_nanos(a) - timespec_to_nanos(b)
}

// StreamInstant representing how long htstamp is ahead of origin, clamped to zero.
// Used as the creation-relative timestamp source for SystemClock and AudioLink fallback paths.
#[inline]
fn htstamp_elapsed(status: &alsa::pcm::Status, origin: alsa::timespec) -> StreamInstant {
    let nanos = timespec_diff_nanos(status.get_htstamp(), origin);
    StreamInstant::from_nanos(nanos.max(0) as u64)
}
