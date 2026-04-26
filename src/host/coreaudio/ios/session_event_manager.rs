//! Monitors AVAudioSession lifecycle events and reports them as stream errors.

use std::{
    ptr::NonNull,
    sync::{Arc, Mutex},
};

use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2_avf_audio::{
    AVAudioSessionMediaServicesWereLostNotification,
    AVAudioSessionMediaServicesWereResetNotification, AVAudioSessionRouteChangeNotification,
    AVAudioSessionRouteChangeReason, AVAudioSessionRouteChangeReasonKey,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNumber, NSString};

use crate::{Error, ErrorKind};

pub(super) type ErrorCallbackMutex = Arc<Mutex<Box<dyn FnMut(Error) + Send>>>;

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
            "audio route changed",
        )),

        AVAudioSessionRouteChangeReason::CategoryChange
        | AVAudioSessionRouteChangeReason::Override
        | AVAudioSessionRouteChangeReason::RouteConfigurationChange => Some(Error::with_message(
            ErrorKind::StreamInvalidated,
            "audio route changed",
        )),

        AVAudioSessionRouteChangeReason::NoSuitableRouteForCategory => Some(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "no suitable audio route for the session category",
        )),

        _ => None,
    }
}

pub(super) struct SessionEventManager {
    observers: Vec<
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
    >,
}

// SAFETY: NSNotificationCenter is thread-safe on iOS. The observer tokens stored here are opaque
// handles used only to call removeObserver in Drop; no data is read or written through them.
unsafe impl Send for SessionEventManager {}
unsafe impl Sync for SessionEventManager {}

impl SessionEventManager {
    pub(super) fn new(error_callback: ErrorCallbackMutex) -> Self {
        let nc = NSNotificationCenter::defaultCenter();
        let mut observers = Vec::new();

        {
            let cb = error_callback.clone();
            let block = RcBlock::new(move |notif: NonNull<NSNotification>| {
                if let Some(err) = unsafe { route_change_error(notif.as_ref()) } {
                    if let Ok(mut cb) = cb.lock() {
                        cb(err);
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
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                if let Ok(mut cb) = cb.lock() {
                    cb(Error::with_message(
                        ErrorKind::DeviceNotAvailable,
                        "audio media services were lost",
                    ));
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
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                if let Ok(mut cb) = cb.lock() {
                    cb(Error::with_message(
                        ErrorKind::StreamInvalidated,
                        "audio media services were reset",
                    ));
                }
            });
            if let Some(name) = unsafe { AVAudioSessionMediaServicesWereResetNotification } {
                let observer = unsafe {
                    nc.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
                };
                observers.push(observer);
            }
        }

        Self { observers }
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
