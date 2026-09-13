//! Pure arithmetic shared by WASAPI rendering and host-independent tests.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderOutcome {
    Submitted,
    Skipped,
}

pub(crate) fn exclusive_timestamp(
    snapshot: crate::StreamInstant,
    now: crate::StreamInstant,
    queued: std::time::Duration,
) -> crate::StreamTimestamp {
    // Some exclusive endpoints provide a zero QPC snapshot until playback advances.
    // Position/frequency still determine queued duration; only its absolute anchor
    // is unavailable. Estimate that startup anchor using the invocation's QPC.
    let anchor = if snapshot == crate::StreamInstant::new(0, 0) {
        now
    } else {
        snapshot
    };
    crate::StreamTimestamp {
        callback: now,
        // A stale snapshot cannot predict new output delivery before invocation.
        device: (anchor + queued).max(now),
    }
}

pub(crate) fn start_after_prime(outcome: RenderOutcome) -> bool {
    outcome == RenderOutcome::Submitted
}

pub(crate) fn retry_alignment(attempt: usize, unaligned: bool) -> bool {
    attempt == 0 && unaligned
}

pub(crate) fn packet_frames(exclusive: bool, capacity: u32, padding: u32) -> u32 {
    if exclusive {
        capacity
    } else {
        capacity.saturating_sub(padding)
    }
}

pub(crate) fn played_frames(position: u64, rate: u32, frequency: u64) -> u64 {
    if frequency == 0 {
        return 0;
    }
    (position as u128 * rate as u128 / frequency as u128).min(u64::MAX as u128) as u64
}

pub(crate) fn aligned_period(frames: u32, rate: u32) -> Option<i64> {
    if frames == 0 || rate == 0 {
        return None;
    }
    Some(((frames as u64 * 10_000_000 + rate as u64 / 2) / rate as u64).max(1) as i64)
}

pub(crate) fn submission_end(written: u64, played: u64, frames: u32) -> u64 {
    written.max(played).saturating_add(frames as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_qpc_startup_snapshots_use_the_current_clock_epoch() {
        use crate::StreamInstant;
        use std::time::Duration;
        let zero = StreamInstant::new(0, 0);
        let now = StreamInstant::new(1830, 0);
        // The reported priming and first post-start snapshots both had QPC zero.
        let priming = exclusive_timestamp(zero, now, Duration::ZERO);
        assert_eq!(priming.callback, now);
        assert_eq!(priming.device, now);
        let next_now = now + Duration::from_millis(1);
        let first_running = exclusive_timestamp(zero, next_now, Duration::from_millis(10));
        assert_eq!(first_running.callback, next_now);
        assert_eq!(first_running.device, next_now + Duration::from_millis(10));
        assert!(first_running.device > priming.device);
    }

    #[test]
    fn valid_qpc_snapshot_anchors_playback_but_not_callback() {
        use crate::StreamInstant;
        use std::time::Duration;
        let snapshot = StreamInstant::new(1830, 0);
        let now = snapshot + Duration::from_millis(2);
        let timestamp = exclusive_timestamp(snapshot, now, Duration::from_millis(10));
        assert_eq!(timestamp.callback, now);
        assert_eq!(timestamp.device, snapshot + Duration::from_millis(10));
        // A stale prediction or underrun cannot put newly supplied output in the past.
        let stale = exclusive_timestamp(snapshot, now, Duration::from_millis(1));
        assert_eq!(stale.callback, now);
        assert_eq!(stale.device, now);
    }

    #[test]
    fn priming_uses_submission_not_a_later_skip_flag() {
        for later_skip_flag in [false, true] {
            let _ = later_skip_flag;
            assert!(!start_after_prime(RenderOutcome::Skipped));
            assert!(start_after_prime(RenderOutcome::Submitted));
        }
    }

    #[test]
    fn alignment_is_rounded_and_retry_is_bounded() {
        assert_eq!(aligned_period(0, 48000), None);
        assert_eq!(aligned_period(256, 0), None);
        for rate in [44100, 48000, 96000] {
            for frames in [128, 256, 512, 1024] {
                let period = aligned_period(frames, rate).unwrap() as u64;
                assert_eq!(
                    (period * rate as u64 + 5_000_000) / 10_000_000,
                    frames as u64
                );
            }
        }
        assert!(retry_alignment(0, true));
        assert!(!retry_alignment(1, true));
        assert!(!retry_alignment(0, false));
    }

    #[test]
    fn silence_remains_queued_on_resume_but_does_not_extend_real_drain() {
        let real = submission_end(0, 0, 256);
        let silent = submission_end(real, 128, 256);
        assert_eq!(real, 256);
        assert_eq!(silent, 512);
        let resumed = submission_end(silent, 256, 256);
        assert_eq!(resumed, 768);
        assert_eq!(submission_end(0, 0, 256), 256); // reset restarts the epoch
        assert_eq!(submission_end(resumed, 1024, 256), 1280); // underrun rebases
        assert_eq!(submission_end(u64::MAX, 0, 256), u64::MAX);
    }

    #[test]
    fn exclusive_packets_are_always_full() {
        for padding in [0, 1, 128, 256, u32::MAX] {
            assert_eq!(packet_frames(true, 256, padding), 256);
        }
        assert_eq!(packet_frames(false, 256, 128), 128);
        assert_eq!(packet_frames(false, 256, 257), 0);
    }

    #[test]
    fn clock_units_are_not_assumed_to_be_frames() {
        assert_eq!(played_frames(10_000_000, 48_000, 10_000_000), 48_000);
        assert_eq!(played_frames(44_100, 44_100, 44_100), 44_100);
        assert_eq!(played_frames(0, 48_000, 10_000_000), 0);
        assert_eq!(played_frames(u64::MAX, u32::MAX, 1), u64::MAX);
    }
}
