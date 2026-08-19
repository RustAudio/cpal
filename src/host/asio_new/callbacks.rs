use super::session::Session;
use super::simplex::{In, Out, Simplex};
use crate::ErrorKind::*;
use crate::*;
use azo::sys::{
    AsioMessage, Bool, BufferSwitch, BufferSwitchTimeInfo, Callbacks as Pointers, MessageSelector,
    SampleRateDidChange, Time,
};
use closure_ffi::BareFnMutSync;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::ffi::c_long;
use std::fmt::{self, Debug};
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::sync::Arc;
use tap::{Conv, Pipe};

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
    MessageSelector::OVERLOAD
];

type Bare<T> = BareFnMutSync<'static, T>;

#[derive(Debug)]
pub struct Callbacks {
    pointers: Pointers,
    closures: Option<Closures>,
    _marker : PhantomPinned
}

impl Callbacks {
    pub const fn pointers(&self) -> &Pointers {
        &self.pointers
    }

    pub fn populate(self: Pin<&mut Self>, context: context_type!()) {
        // SAFETY:
        // The ffi closures relying on this pin are (re-)created here
        let mutable = unsafe { Pin::get_unchecked_mut(self) };

        // If `self` has been populated before, then replacing the closures
        // would cause the fn pointers to dangle, which would be UB because
        // unlike regular raw pointers, fn pointers are implicitly non-nullable
        mutable.pointers = Pointers::noop();

        // it is now safe to overwrite the closures
        mutable.closures = context
            .pipe(Mutex::new)
            .pipe(Closures::new)
            .pipe(Some);

        // This makes `self` self-referential, which is why it needs to be pinned
        mutable.pointers = mutable
            .closures
            .as_ref()
            .unwrap() // infallible, as it was just assigned
            .to_pointers();
    }
}

impl Default for Callbacks {
    fn default() -> Self {
        Self {
            pointers: Pointers::noop(),
            closures: None,
            _marker : PhantomPinned
        }
    }
}

struct Closures {
    buffer_switch          : Bare<BufferSwitch>,
    sample_rate_did_change : Bare<SampleRateDidChange>,
    asio_message           : Bare<AsioMessage>,
    buffer_switch_time_info: Bare<BufferSwitchTimeInfo>
}

impl Closures {
    fn new(context: Mutex<context_type!()>) -> Self {
        let arc1 = Arc::new(context);
        let arc2 = Arc::clone(&arc1);
        let arc3 = Arc::clone(&arc1);
        let arc4 = Arc::clone(&arc1);

        Self {
            sample_rate_did_change : create_sample_rate_did_change (arc1),
            asio_message           : create_asio_message           (arc2),
            buffer_switch_time_info: create_buffer_switch_time_info(arc3),
            buffer_switch          : create_buffer_switch          (arc4),
        }
    }

    fn to_pointers(&self) -> Pointers {
        Pointers {
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
        context.process_buffers(direct_process, buffer_side as _, instant);
    };

    Bare::new_system(closure)
}

fn create_sample_rate_did_change(context_handle: context_handle_type!()) -> Bare<SampleRateDidChange> {
    let closure = move |new_rate| {
        let mut context = context_handle.lock();
        // `ErrorKind::Other` because this isn't fatal
        context.throw(Other, format!("ASIO driver changed the sample rate (to {new_rate})"));
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
                context.throw(StreamInvalidated, "ASIO driver requested a reset");
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
            },

            MessageSelector::LATENCIES_CHANGED => {
                context.update_latencies();
                Bool::TRUE.0
            }

            MessageSelector::SUPPORTS_TIME_INFO => {
                Bool::TRUE.0
            }

            _ => Bool::FALSE.0
        }
    };

    Bare::new_system(closure)
}

fn create_buffer_switch_time_info(context_handle: context_handle_type!()) -> Bare<BufferSwitchTimeInfo> {
    let closure = move |time_ptr: *mut Time, buffer_side: c_long, direct_process: Bool| {
        let mut context = context_handle.lock();

        match unsafe { time_ptr.as_ref() } {
            Some(time) => context.process_buffers(direct_process, buffer_side as _, StreamInstant::from_millis(time.time_info.system_time as _)),
            None       => context.throw(BackendError, "ASIO driver produced invalid time pointer")
        }

        time_ptr
    };

    Bare::new_system(closure)
}

pub struct Context<DataCb, ErrorCb> {
    pub session    : Arc<Session>,
    pub data_cb    : DataCb,
    pub error_cb   : ErrorCb,
    pub sample_rate: SampleRate,
    pub simplex_in : Simplex<In>,
    pub simplex_out: Simplex<Out>,
}

impl<DataCb, ErrorCb> Context<DataCb, ErrorCb>
where
    DataCb : FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
    ErrorCb: FnMut(Error) + Send + 'static
{
    fn process_buffers(&mut self, direct_process: Bool, buffer_side: usize, cb_time: StreamInstant) {
        // The ASIO spec contexts `direct_process` to always be true on Windows,
        // and dropped support for other platforms. But just in case:
        if direct_process == Bool::FALSE {
            self.throw(RealtimeDenied, "ASIO driver prohibits processing within the buffer switch callback");
            return;
        }

        let     data_in   = self.simplex_in.data(buffer_side);
        let mut data_out  = self.simplex_out.data(buffer_side);
        let callback_info = self.create_cb_info(cb_time);

        self.simplex_in.interleave(buffer_side);
        (self.data_cb)(&data_in, &mut data_out, &callback_info);
        self.simplex_out.deinterleave(buffer_side);
    }

    fn create_cb_info(&self, cb_time: StreamInstant) -> DuplexCallbackInfo {
        let time_in  = cb_time - self.simplex_in .latency;
        let time_out = cb_time + self.simplex_out.latency;

        [time_in, time_out]
        .map (| dev_time | StreamTimestamp { callback: cb_time, device: dev_time })
        .map (| timestamp| CallbackInfo::new(timestamp, false))
        .pipe(|[in_, out]| DuplexCallbackInfo::new(in_, out))
    }

    fn update_latencies(&mut self) {
        match self.session.latencies(self.sample_rate) {
            Ok([latency_in, latency_out]) => {
                self.simplex_in .latency = latency_in;
                self.simplex_out.latency = latency_out;
            }
            Err(error) => {
                (self.error_cb)(error);
            },
        }
    }
    fn throw(&mut self, kind: ErrorKind, message: impl Into<Cow<'static, str>>) {
        (self.error_cb)(Error::with_message(kind, message));
    }
}
