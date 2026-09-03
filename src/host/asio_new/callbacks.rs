use super::*;
use azo::sys::*;
use closure_ffi::BareFnMutSync;
use std::ffi::c_long;
use std::fmt::{self, Debug};
use std::marker::PhantomPinned;
use std::sync::Mutex;

const ASIO_VERSION_MAJOR: c_long = 2; // = 2.x

const SUPPORTED_MESSAGE_SELECTORS: &[MessageSelector] = &[
    MessageSelector::SELECTOR_SUPPORTED,
    MessageSelector::ENGINE_VERSION,
    MessageSelector::RESET_REQUEST,
    MessageSelector::BUFFER_SIZE_CHANGE,
    MessageSelector::RESYNC_REQUEST,
    // MessageSelector::LATENCIES_CHANGED,
    MessageSelector::SUPPORTS_TIME_INFO,
    MessageSelector::SUPPORTS_TIME_CODE,
    MessageSelector::OVERLOAD
];

type Bare<T> = BareFnMutSync<'static, T>;

#[derive(Debug)]
pub struct Container {
    pointers: Callbacks,
    closures: Closures,
    _marker : PhantomPinned
}

impl Container {
    pub const fn pointers(&self) -> &Callbacks {
        &self.pointers
    }

    pub fn prime(
        self: Pin<&mut Self>,
        session    : Arc<Session>,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
        simplex_in : simplex::WithScratch,
        simplex_out: simplex::WithScratch
    ) {
        // SAFETY:
        // `self` is not self-referential yet (but we're changing that now)
        let mutable = unsafe { Pin::get_unchecked_mut(self) };

        let error_cb1 = error_cb
            .pipe(Mutex::new)
            .pipe(Arc::new);

        let error_cb2 = Arc::clone(&error_cb1);
        let error_cb3 = Arc::clone(&error_cb1);
        let error_cb4 = Arc::clone(&error_cb1);

        mutable.closures.sample_rate_did_change  = create_sample_rate_did_change (error_cb2);
        mutable.closures.asio_message            = create_asio_message           (error_cb3);
        mutable.closures.buffer_switch_time_info = create_buffer_switch_time_info(error_cb4, data_cb, simplex_in, simplex_out);
        mutable.closures.buffer_switch           = create_buffer_switch          (error_cb1, session, mutable.closures.buffer_switch_time_info.bare());

        mutable.pointers.buffer_switch           = mutable.closures.buffer_switch          .bare();
        mutable.pointers.buffer_switch_time_info = mutable.closures.buffer_switch_time_info.bare();
        mutable.pointers.sample_rate_did_change  = mutable.closures.sample_rate_did_change .bare();
        mutable.pointers.asio_message            = mutable.closures.asio_message           .bare();
    }
}

impl Default for Container {
    fn default() -> Self {
        Self {
            pointers: Callbacks::noop(),
            closures: Closures::noop(),
            _marker : PhantomPinned
        }
    }
}

pub struct Closures {
    buffer_switch          : Bare<BufferSwitch>,
    sample_rate_did_change : Bare<SampleRateDidChange>,
    asio_message           : Bare<AsioMessage>,
    buffer_switch_time_info: Bare<BufferSwitchTimeInfo>
}

impl Closures {
    fn noop() -> Self {
        Self {
            buffer_switch          : Bare::new_system(|_, _| ()),
            sample_rate_did_change : Bare::new_system(|_| ()),
            asio_message           : Bare::new_system(|_, _, _, _| 0),
            buffer_switch_time_info: Bare::new_system(|time, _, _| time),
        }
    }
}

impl Debug for Closures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(Closures))
            .field("buffer_switch", &self.buffer_switch.bare())
            .field("sample_rate_did_change", &self.sample_rate_did_change.bare())
            .field("asio_message", &self.asio_message.bare())
            .field("buffer_switch_time_info", &self.buffer_switch_time_info.bare())
            .finish()
    }
}

/// forwards to bsti by retrieving [`Time`] the old way
pub fn create_buffer_switch(
    error_cb: Arc<Mutex<error_cb_type!()>>,
    session : Arc<Session>,
    bsti_ptr: BufferSwitchTimeInfo
) -> Bare<BufferSwitch> {
    let closure =
        move |buf_idx: c_long, direct_process: Bool| match session.driver.sample_position() {
            Ok(pos) => {
                let mut time = create_minimal_azo_time(&pos);
                unsafe { bsti_ptr(&raw mut time, buf_idx, direct_process); }
            }
            Err(error) => throw(
                &error_cb,
                create_report(&session.driver, error, "sample_position")
            )
        };

    Bare::new_system(closure)
}

pub fn create_sample_rate_did_change(error_cb: Arc<Mutex<error_cb_type!()>>) -> Bare<SampleRateDidChange> {
    let closure = move |new_rate| {
        // `ErrorKind::Other` because this isn't fatal
        throw(&error_cb, Error::with_message(Other, format!("ASIO driver changed the sample rate (to {new_rate})")));
    };

    Bare::new_system(closure)
}

pub fn create_asio_message(error_cb: Arc<Mutex<error_cb_type!()>>) -> Bare<AsioMessage> {
    let closure = move |selector, value, _message, _opt| {
        match selector {
            MessageSelector::SELECTOR_SUPPORTED =>
                SUPPORTED_MESSAGE_SELECTORS
                .contains(&MessageSelector(value))
                .conv::<Bool>()
                .0,

            MessageSelector::ENGINE_VERSION =>
                ASIO_VERSION_MAJOR,

            MessageSelector::RESET_REQUEST => {
                throw(&error_cb, Error::with_message(StreamInvalidated, "ASIO driver requested a reset"));
                Bool::TRUE.0
            }

            MessageSelector::BUFFER_SIZE_CHANGE => {
                if value.is_negative() {
                    throw(&error_cb, Error::with_message(BackendError, format!("ASIO driver reported invalid buffer size: {value}")));
                    Bool::FALSE
                } else {
                    throw(&error_cb, Error::with_message(StreamInvalidated, format!("ASIO driver changed its buffer size (to {value})")));
                    Bool::TRUE
                }
                .0
            }

            MessageSelector::RESYNC_REQUEST => {
                throw(&error_cb, Error::with_message(StreamInvalidated, "ASIO driver requested a resync"));
                Bool::TRUE.0
            },

            MessageSelector::SUPPORTS_TIME_INFO =>
                Bool::TRUE.0,

            _ => Bool::FALSE.0
        }
    };

    Bare::new_system(closure)
}

pub fn create_buffer_switch_time_info(
    error_callback   : Arc<Mutex<error_cb_type!()>>,
    mut data_callback: data_cb_type!(),
    mut simplex_in   : simplex::WithScratch,
    mut simplex_out  : simplex::WithScratch
) -> Bare<BufferSwitchTimeInfo> {
    let closure = move |time: *mut Time, buf_idx: c_long, direct_process: Bool| {
        // The ASIO spec claims `direct_process` to always be true on Windows,
        // and dropped support for other platforms. But just in case:
        if direct_process != Bool::TRUE {
            throw(&error_callback, Error::with_message(RealtimeDenied, "ASIO driver prohibits processing within the buffer switch callback"));
            return time;
        }

        let callback_info = unsafe { time.read() }
            .time_info
            .system_time
            .cast_unsigned()
            .pipe(StreamInstant::from_millis)
            .pipe(|instant| StreamTimestamp { callback: instant, device: instant })
            .pipe(|stamp| CallbackInfo::new(stamp, false))
            .pipe(|cbi| DuplexCallbackInfo::new(cbi, cbi));

        simplex_in.interleave(buf_idx as _);
        data_callback(&simplex_in.data(buf_idx as _), &mut simplex_out.data(buf_idx as _), &callback_info);
        simplex_out.deinterleave(buf_idx as _);

        time
    };

    Bare::new_system(closure)
}

fn throw(error_cb: &Mutex<error_cb_type!()>, error: Error) {
    error_cb.lock().expect("mutex poisoned")(error);
}