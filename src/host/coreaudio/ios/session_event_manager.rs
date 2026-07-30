//! Monitors AVAudioSession lifecycle events, recovering the stream from an interruption and
//! reporting the rest as stream errors.

use std::{
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
};

use block2::RcBlock;
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionInterruptionNotification, AVAudioSessionInterruptionOptionKey,
    AVAudioSessionInterruptionOptions, AVAudioSessionInterruptionType,
    AVAudioSessionInterruptionTypeKey, AVAudioSessionMediaServicesWereLostNotification,
    AVAudioSessionMediaServicesWereResetNotification, AVAudioSessionRouteChangeNotification,
    AVAudioSessionRouteChangeReason, AVAudioSessionRouteChangeReasonKey,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNumber, NSString};

use super::{input_latency_frames, output_latency_frames, StreamInner};
use crate::{
    host::{emit_error, latch::Latch, ErrorCallbackArc},
    Error, ErrorKind,
};

/// Runs `f` against the stream, if it is still alive and its lock is intact.
fn with_stream(stream: &Weak<Mutex<StreamInner>>, f: impl FnOnce(&mut StreamInner)) {
    if let Some(inner) = stream.upgrade() {
        if let Ok(mut inner) = inner.lock() {
            f(&mut inner);
        }
    }
}

/// Reads the number stored under `key` in a notification's `userInfo`.
fn user_info_number(notification: &NSNotification, key: Option<&NSString>) -> Option<usize> {
    let user_info = notification.userInfo()?;
    let value = user_info.objectForKey(key?)?;
    Some(value.downcast::<NSNumber>().ok()?.unsignedIntegerValue())
}

/// Shared buffer-depth value to refresh on route changes, paired with `is_input` to select the
/// input or output latency. `true` means an input stream.
type LatencyRefresh = (Arc<AtomicUsize>, bool);

fn route_change_error(notification: &NSNotification) -> Option<Error> {
    let key = unsafe { AVAudioSessionRouteChangeReasonKey };
    let reason = AVAudioSessionRouteChangeReason(user_info_number(notification, key)?);
    match reason {
        AVAudioSessionRouteChangeReason::OldDeviceUnavailable => Some(Error::with_message(
            ErrorKind::DeviceChanged,
            "Audio route changed",
        )),

        AVAudioSessionRouteChangeReason::CategoryChange
        | AVAudioSessionRouteChangeReason::Override
        | AVAudioSessionRouteChangeReason::RouteConfigurationChange => Some(Error::with_message(
            ErrorKind::StreamInvalidated,
            "Audio route changed",
        )),

        AVAudioSessionRouteChangeReason::NoSuitableRouteForCategory => Some(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "No suitable audio route for the session category",
        )),

        _ => None,
    }
}

pub(super) struct SessionEventManager {
    latch: Latch,
    observers: Vec<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    >,
}

// SAFETY: NSNotificationCenter is thread-safe on iOS. The observer tokens stored here are opaque
// handles used only to call removeObserver in Drop; no data is read or written through them.
unsafe impl Send for SessionEventManager {}
unsafe impl Sync for SessionEventManager {}

impl SessionEventManager {
    pub(super) fn new(
        error_callback: ErrorCallbackArc,
        latch: Latch,
        latency_refresh: Option<LatencyRefresh>,
        stream: Weak<Mutex<StreamInner>>,
    ) -> Self {
        let nc = NSNotificationCenter::defaultCenter();
        let mut observers = Vec::new();
        let waiter = latch.waiter();

        // The OS stops the unit itself, and the session it stops is inactive on the way back.
        {
            let w = waiter.clone();
            let stream = stream.clone();
            let block = RcBlock::new(move |notif: NonNull<NSNotification>| {
                if !w.is_released() {
                    return;
                }
                let notif = unsafe { notif.as_ref() };
                let interruption_type_key = unsafe { AVAudioSessionInterruptionTypeKey };
                let Some(kind) = user_info_number(notif, interruption_type_key) else {
                    return;
                };
                if AVAudioSessionInterruptionType(kind) == AVAudioSessionInterruptionType::Began {
                    with_stream(&stream, StreamInner::stop_for_interruption);
                    return;
                }
                let interruption_option_key = unsafe { AVAudioSessionInterruptionOptionKey };
                let options = AVAudioSessionInterruptionOptions(
                    user_info_number(notif, interruption_option_key).unwrap_or(0),
                );
                if !options.contains(AVAudioSessionInterruptionOptions::ShouldResume) {
                    return;
                }
                let session = unsafe { AVAudioSession::sharedInstance() };
                if unsafe { session.setActive_error(true) }.is_ok() {
                    with_stream(&stream, StreamInner::resume_after_interruption);
                }
            });
            if let Some(name) = unsafe { AVAudioSessionInterruptionNotification } {
                let observer = unsafe {
                    nc.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
                };
                observers.push(observer);
            }
        }

        {
            let cb = error_callback.clone();
            let w = waiter.clone();
            let block = RcBlock::new(move |notif: NonNull<NSNotification>| {
                if w.is_released() {
                    // The route may have changed the active device; recompute the buffer depth so
                    // capture/playback timestamps track the new latency.
                    if let Some((frames, is_input)) = &latency_refresh {
                        let depth = if *is_input {
                            input_latency_frames()
                        } else {
                            output_latency_frames()
                        };
                        frames.store(depth, Ordering::Relaxed);
                    }
                    let notif = unsafe { notif.as_ref() };
                    if let Some(err) = route_change_error(notif) {
                        emit_error(&cb, err);
                    }
                }
            });
            if let Some(name) = unsafe { AVAudioSessionRouteChangeNotification } {
                let observer = unsafe {
                    nc.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
                };
                observers.push(observer);
            }
        }

        {
            let cb = error_callback.clone();
            let w = waiter.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                if w.is_released() {
                    emit_error(
                        &cb,
                        Error::with_message(
                            ErrorKind::DeviceNotAvailable,
                            "Audio media services were lost",
                        ),
                    );
                }
            });
            if let Some(name) = unsafe { AVAudioSessionMediaServicesWereLostNotification } {
                let observer = unsafe {
                    nc.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
                };
                observers.push(observer);
            }
        }

        {
            let cb = error_callback.clone();
            let w = waiter;
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                if w.is_released() {
                    emit_error(
                        &cb,
                        Error::with_message(
                            ErrorKind::StreamInvalidated,
                            "Audio media services were reset",
                        ),
                    );
                }
            });
            if let Some(name) = unsafe { AVAudioSessionMediaServicesWereResetNotification } {
                let observer = unsafe {
                    nc.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
                };
                observers.push(observer);
            }
        }

        Self { latch, observers }
    }

    pub(super) fn signal_ready(&self) {
        self.latch.release();
    }
}

impl Drop for SessionEventManager {
    fn drop(&mut self) {
        let nc = NSNotificationCenter::defaultCenter();
        for observer in &self.observers {
            unsafe { nc.removeObserver(observer.as_ref()) };
        }
    }
}
