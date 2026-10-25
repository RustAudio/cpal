use std::sync::atomic::{AtomicI32, Ordering};

pub(crate) fn update_mixer_bursts_after_resize(
    mixer_bursts: &AtomicI32,
    new_mixer_bursts: i32,
    requested_frames: i32,
    actual_frames: i32,
) -> bool {
    if requested_frames <= 0 || actual_frames < requested_frames {
        return false;
    }

    mixer_bursts.store(new_mixer_bursts, Ordering::Relaxed);
    true
}

#[cfg(test)]
mod tests {
    use super::update_mixer_bursts_after_resize;
    use std::sync::atomic::{AtomicI32, Ordering};

    #[test]
    fn records_the_new_count_when_the_requested_size_is_reached() {
        let mixer_bursts = AtomicI32::new(2);

        assert!(update_mixer_bursts_after_resize(&mixer_bursts, 3, 288, 288));
        assert_eq!(mixer_bursts.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn records_the_new_count_when_audio_rounds_up_the_size() {
        let mixer_bursts = AtomicI32::new(2);

        assert!(update_mixer_bursts_after_resize(&mixer_bursts, 3, 288, 384));
        assert_eq!(mixer_bursts.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn keeps_the_old_count_when_the_requested_size_was_not_reached() {
        let mixer_bursts = AtomicI32::new(2);

        assert!(!update_mixer_bursts_after_resize(
            &mixer_bursts,
            3,
            288,
            192
        ));
        assert_eq!(mixer_bursts.load(Ordering::Relaxed), 2);
    }
}
