use std::time::Duration;
use super::Session;
use std::ffi::c_long;
use std::fmt::{self, Debug};
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::sync::Arc;
use azo::sys::{AsioMessage, Bool, BufferSwitch, BufferSwitchTimeInfo, Callbacks as FnPointers, SampleRateDidChange, Time};
use closure_ffi::BareFnMutSync;
use parking_lot::Mutex;
use tap::Pipe;

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

    pub fn new(
        session    : Arc<Session>,
        data_cb    : data_cb_type!(),
        error_cb   : error_cb_type!(),
        latencies  : [Duration; 2]
    ) -> Pin<Box<Self>> {
        todo!()
    }
}

/// workaround until `#![feature(type_alias_impl_trait)]` is stabilized
macro_rules! context_handle_type {
    () => { Arc<Mutex<Context>> };
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
        todo!()
    };

    Bare::new_system(closure)
}

fn create_sample_rate_did_change(context_handle: context_handle_type!()) -> Bare<SampleRateDidChange> {
    let closure = move |new_rate| {
        todo!()
    };

    Bare::new_system(closure)
}

fn create_asio_message(context_handle: context_handle_type!()) -> Bare<AsioMessage> {
    let closure = move |selector, value, _message, _opt| {
        todo!()
    };

    Bare::new_system(closure)
}

fn create_buffer_switch_time_info(context_handle: context_handle_type!()) -> Bare<BufferSwitchTimeInfo> {
    let closure = move |time_ptr: *mut Time, buffer_side: c_long, direct_process: Bool| {
        todo!()
    };

    Bare::new_system(closure)
}

struct Context {
	
}