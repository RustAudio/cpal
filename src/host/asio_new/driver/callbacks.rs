use azo::sys::{
    AsioMessage, Bool, BufferSwitch, BufferSwitchTimeInfo, Callbacks as FnPointers,
    MessageSelector, SampleRateDidChange, Time,
};
use closure_ffi::BareFnMutSync;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::ffi::c_long;
use std::fmt::{self, Debug};
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tap::{Conv, Pipe};

use crate::ErrorKind::*;
use crate::host::asio_new::buffer::{In, Out, Buffer};
use crate::*;

use super::Session;

/// workaround until `#![feature(type_alias_impl_trait)]` is stabilized
macro_rules! context_handle_type {
    () => { Arc<Mutex<Context<data_cb_type!(), error_cb_type!()>>> };
}

const ASIO_VERSION_MAJOR: c_long = 2; // = 2.x

const SUPPORTED_MESSAGE_SELECTORS: &[MessageSelector] = &[
    MessageSelector::SELECTOR_SUPPORTED,
    MessageSelector::ENGINE_VERSION,
    MessageSelector::RESET_REQUEST,
    MessageSelector::BUFFER_SIZE_CHANGE,
    MessageSelector::RESYNC_REQUEST,
    MessageSelector::LATENCIES_CHANGED,
    MessageSelector::SUPPORTS_TIME_INFO,
    MessageSelector::SUPPORTS_TIME_CODE,
    MessageSelector::OVERLOAD,
];

type Bare<T> = BareFnMutSync<'static, T>;

#[derive(Debug)]
pub struct Callbacks {
    pointers: FnPointers,
    closures: Closures,
    _marker : PhantomPinned,
}

impl Callbacks {
    pub const fn fn_pointers(&self) -> &FnPointers {
        &self.pointers
    }

    #[expect(clippy::clone_on_ref_ptr, reason = "sufficiently obvious from name")]
    #[expect(clippy::type_complexity, reason = "see `context_handle_type!()`")]
    pub fn new(
        session    : Arc<Session>,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
        latencies  : [Duration; 2]
    ) -> (Pin<Box<Self>>, context_handle_type!()) {
        let context_handle = Context {
            session,
            data_cb,
            error_cb,
            latencies,
            buffers: None // we only get the buffer pointers *after* we handed over the callback pointers
        }
        .pipe(Mutex::new)
        .pipe(Arc::new);

        // We can only assign the fn pointers *after* the struct is pinned, but unlike regular raw pointers,
        // fn pointers are implicitly non-nullable, so letting them dangle even momentarily is UB.
        // We avoid this by filling in static no-op functions as placeholders.
        let mut pinned = Self {
            pointers: FnPointers::NOOP,
            closures: Closures::new(context_handle.clone()),
            _marker: PhantomPinned
        }
        .pipe(Box::pin);
    
        // SAFETY:
        // We just created the struct, so there is nothing relying on it being pinned yet.
        let mutable = unsafe { pinned.as_mut().get_unchecked_mut() };
        mutable.pointers = mutable.closures.to_pointers();
        
        (pinned, context_handle)
    }
}

struct Closures {
    buffer_switch          : Bare<BufferSwitch>,
    sample_rate_did_change : Bare<SampleRateDidChange>,
    asio_message           : Bare<AsioMessage>,
    buffer_switch_time_info: Bare<BufferSwitchTimeInfo>,
}

impl Closures {
    #[expect(clippy::clone_on_ref_ptr, reason = "sufficiently obvious from name")]
    fn new(context_handle: context_handle_type!()) -> Self {
        let context_handle2 = context_handle.clone();
        let context_handle3 = context_handle.clone();
        let context_handle4 = context_handle.clone();

        Self {
            sample_rate_did_change : create_sample_rate_did_change (context_handle),
            asio_message           : create_asio_message           (context_handle2),
            buffer_switch_time_info: create_buffer_switch_time_info(context_handle3),
            buffer_switch          : create_buffer_switch          (context_handle4),
        }
    }

    fn to_pointers(&self) -> FnPointers {
        FnPointers {
            buffer_switch          : self.buffer_switch          .bare(),
            buffer_switch_time_info: self.buffer_switch_time_info.bare(),
            sample_rate_did_change : self.sample_rate_did_change .bare(),
            asio_message           : self.asio_message           .bare(),
        }
    }
}

impl Debug for Closures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(Closures))
            .field("buffer_switch"          , &self.buffer_switch          .bare())
            .field("sample_rate_did_change" , &self.sample_rate_did_change .bare())
            .field("asio_message"           , &self.asio_message           .bare())
            .field("buffer_switch_time_info", &self.buffer_switch_time_info.bare())
            .finish()
    }
}

fn create_buffer_switch(context_handle: context_handle_type!()) -> Bare<BufferSwitch> {
    let closure = move |buffer_side: c_long, direct_process: Bool| {
        let mut context = context_handle.lock();

        let instant = context.session.now();
        context.buffer_switch_either(direct_process, buffer_side as _, instant);
    };

    Bare::new_system(closure)
}

fn create_sample_rate_did_change(context_handle: context_handle_type!()) -> Bare<SampleRateDidChange> {
    let closure = move |new_rate| {
        // `ErrorKind::Other` because this is technically recoverable
        context_handle.lock().throw(Other, format!("ASIO driver changed the sample rate (to {new_rate})"));
    };

    Bare::new_system(closure)
}

fn create_asio_message(context_handle: context_handle_type!()) -> Bare<AsioMessage> {
    let closure = move |selector, value, _message, _opt| {
        let mut context = context_handle.lock();

        match selector {
            MessageSelector::SELECTOR_SUPPORTED => {
                SUPPORTED_MESSAGE_SELECTORS
                    .contains(&MessageSelector(value))
                    .conv::<Bool>()
                    .0
            }

            MessageSelector::ENGINE_VERSION => {
                ASIO_VERSION_MAJOR
            }

            MessageSelector::RESET_REQUEST => {
                context.throw(StreamInvalidated, "ASIO driver requested a reset. Drop and re-create the stream");
                Bool::TRUE.0
            }

            MessageSelector::BUFFER_SIZE_CHANGE => {
                if value.is_negative() {
                    context.throw(BackendError, format!("ASIO driver reported invalid buffer size: {value}"));
                    Bool::FALSE.0
                } else {
                    context.throw(StreamInvalidated, format!("ASIO driver changed its buffer size (to {value})"));
                    Bool::TRUE.0
                }
            }

            MessageSelector::RESYNC_REQUEST => {
                context.throw(StreamInvalidated, "ASIO driver requested a resync");
                Bool::TRUE.0
            }

            MessageSelector::LATENCIES_CHANGED => {
                context.update_cached_latencies();
                Bool::TRUE.0
            }

            MessageSelector::SUPPORTS_TIME_INFO => {
                Bool::TRUE.0
            }

            _ => Bool::FALSE.0,
        }
    };

    Bare::new_system(closure)
}

fn create_buffer_switch_time_info(context_handle: context_handle_type!()) -> Bare<BufferSwitchTimeInfo> {
    let closure = move |time_ptr: *mut Time, buffer_side: c_long, direct_process: Bool| {
        let mut context = context_handle.lock();

        let Some(time) = (unsafe { time_ptr.as_ref() })
        else {
            context.throw(BackendError, "ASIO driver produced invalid time pointer");
            return time_ptr;
        };

        let now = StreamInstant::from_nanos(time.time_info.system_time.conv::<i64>() as _);
        context.buffer_switch_either(direct_process, buffer_side as _, now);

        time_ptr
    };

    Bare::new_system(closure)
}

pub struct Context<DataCb, ErrorCb> {
    session  : Arc<Session>,
    error_cb : ErrorCb,
    data_cb  : DataCb,
    latencies: [Duration; 2],
    buffers  : Option<Buffers>,
}

pub struct Buffers {
    in_: Buffer<In>,
    out: Buffer<Out>
}

impl<DataCb, ErrorCb> Context<DataCb, ErrorCb>
where
    DataCb : FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
    ErrorCb: FnMut(Error) + Send + 'static,
{
    pub fn set_buffers(&mut self, in_: Buffer<In>, out: Buffer<Out>) {
        self.buffers = Some(Buffers { in_, out });
    }
    
    /// Called by either [`BufferSwitch`] or [`BufferSwitchTimeInfo`]
    fn buffer_switch_either(&mut self, direct_process: Bool, side: usize, cb_time: StreamInstant) {
        let dp = direct_process.try_into().unwrap_or(true); // assume C-style truthiness for compatibility

        self.process_buffers(dp, side, cb_time);
    }

    fn process_buffers(&mut self, direct_process: bool, side: usize, cb_time: StreamInstant) {
        // The ASIO spec claims `direct_process` to always be true on Windows,
        // which is the only platform on which ASIO is currently supported.
        // But just in case:
        if !direct_process {
            self.throw(RealtimeDenied, "ASIO driver prohibits processing within the buffer switch callback");
            return;
        }

        let Some(buffers) = self.buffers.as_mut()
        else {
            self.throw(Other, "BUG! buffer switch callback should not be called before buffers are created");
            return;
        };

        let     data_in   = buffers.in_.data(side);
        let mut data_out  = buffers.out.data(side);
        let callback_info = create_cb_info(cb_time, self.latencies);

        buffers.in_.interleave(side);
        (self.data_cb)(&data_in, &mut data_out, &callback_info);
        buffers.out.deinterleave(side);
    }

    /// latencies are cached because we need them on every buffer cycle
    fn update_cached_latencies(&mut self) {
        match self.session.driver.latencies() {
            Ok(latencies) => self.latencies = latencies,
            Err(error)    => (self.error_cb)(error),
        }
    }

    fn throw(&mut self, kind: ErrorKind, message: impl Into<Cow<'static, str>>) {
        (self.error_cb)(Error::with_message(kind, message));
    }
}

fn create_cb_info(cb_time: StreamInstant, [latency_in, latency_out]: [Duration; 2]) -> DuplexCallbackInfo {
    let time_in  = cb_time.checked_sub(latency_in).unwrap_or(StreamInstant::ZERO); // this underflows during priming for drivers using the legacy `BufferSwitch` callback
    let time_out = cb_time + latency_out;

    [time_in, time_out]
    .map (| dev_time | StreamTimestamp { callback: cb_time, device: dev_time })
    .map (| timestamp| CallbackInfo::new(timestamp, false))
    .pipe(|[in_, out]| DuplexCallbackInfo::new(in_, out))
}