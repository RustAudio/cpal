//! Monitors AVAudioSession lifecycle events and reports them as stream errors.

use std::{
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2_avf_audio::{
    AVAudioSessionMediaServicesWereLostNotification,
    AVAudioSessionMediaServicesWereResetNotification, AVAudioSessionRouteChangeNotification,
    AVAudioSessionRouteChangeReason, AVAudioSessionRouteChangeReasonKey,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNumber, NSString};

use super::{input_latency_frames, output_latency_frames};
use crate::{
    host::{emit_error, latch::Latch, ErrorCallbackArc},
    Error, ErrorKind,
};

/// Shared buffer-depth value to refresh on route changes, paired with `is_input` to select the
/// input or output latency. `true` means an input stream.
type LatencyRefresh = (Arc<AtomicUsize>, bool);

unsafe fn route_change_error(notification: &NSNotification) -> Option<Error> {
    let user_info = notification.userInfo()?;
    let key = AVAudioSessionRouteChangeReasonKey?;
    let dict = unsafe { user_info.cast_unchecked::<NSString, AnyObject>() };
    let value = dict.objectForKey(key)?;
    let number = value.downcast_ref::<NSNumber>()?;
    let reason = AVAudioSessionRouteChangeReason(number.unsignedIntegerValue());
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
    ) -> Self {
        let nc = NSNotificationCenter::defaultCenter();
        let mut observers = Vec::new();
        let waiter = latch.waiter();

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
                    if let Some(err) = unsafe { route_change_error(notif.as_ref()) } {
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
