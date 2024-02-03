use std::{
    mem,
    sync::{Arc, Mutex},
};

use js_sys::{
    encode_uri_component, Atomics, DataView, Float32Array, Function, Int32Array, JsString, Object,
    Reflect, SharedArrayBuffer,
};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AudioContext, AudioDestinationNode, AudioNode, AudioWorkletNode, MessageEvent, Worker,
};

use wasm_bindgen::prelude::*;

use crate::{BuildStreamError, PauseStreamError, PlayStreamError};

use super::map_js_err;

// TODO: minify etc
const AUDIO_WORKLET: &str = include_str!("worklet.js");
const SCHEDULING_WORKER: &str = include_str!("worker.js");

// Float32Array.BYTES_PER_ELEMENT = 4
// Int32Array.BYTES_PER_ELEMENT = 4
const BYTE_SIZE: u32 = 4;

#[derive(PartialEq, Eq, Debug)]
pub(crate) enum BridgePhase {
    /// audio input callback have read the data
    Input = 0,
    /// audio output callback produced the data
    Output = 1,
    /// worklet have read the data and written input
    ReadWrite = 2,
    /// waiter have sent message to main thread to produce new data
    Demand = 3,
}

impl From<i32> for BridgePhase {
    fn from(value: i32) -> Self {
        match value {
            0 => BridgePhase::Input,
            1 => BridgePhase::Output,
            2 => BridgePhase::ReadWrite,
            3 => BridgePhase::Demand,
            _ => panic!(),
        }
    }
}

pub(crate) struct WebAudioBridge {
    /// plays the shared buffer
    worklet: Arc<Mutex<Option<AudioWorkletNode>>>,
    /// once module is added these are connected to worklet
    dst_fut: Arc<Mutex<Vec<Arc<AudioNode>>>>,
    /// worker allows wait on Atomics
    /// TODO: consider refactoring to waitAsync once this issue is resolved
    /// https://bugzilla.mozilla.org/show_bug.cgi?id=1467846
    waiter: Arc<Worker>,
    /// store the shared buffer for all channels and one place to wait on
    buffer: Arc<SharedArrayBuffer>,
    /// data view over floats for converting them into ints and back
    view: Arc<DataView>,
    /// using Int32Array over the given buffer for use with Atomics
    ints: Arc<Int32Array>,
    /// bridge owned buffer with f32
    floats: Arc<Float32Array>,
    /// keep audio context
    ctx: Arc<AudioContext>,
    /// store input callback
    input_cb: Option<Closure<dyn FnMut(JsValue)>>,
    /// store output callback
    output_cb: Option<Closure<dyn FnMut(JsValue)>>,
}

impl WebAudioBridge {
    pub fn new(
        ctx: Arc<AudioContext>,
        channels: u16,
        frames: u32,
    ) -> Result<Self, BuildStreamError> {
        let floats = Float32Array::new_with_length(channels as u32 * frames);

        let view = Arc::new(DataView::new(
            &floats.buffer(),
            0,
            floats.byte_length() as usize,
        ));

        // + the place for BridgePhase
        let bl = floats.byte_length() + BYTE_SIZE;
        let buffer = SharedArrayBuffer::new(bl);

        let ints = Arc::new(Int32Array::new(&buffer));

        let buffer = Arc::new(buffer);
        let floats = Arc::new(floats);

        let dst_fut: Arc<Mutex<Vec<Arc<AudioNode>>>> = Arc::new(Mutex::new(vec![]));
        // try creating new worklet or add module and reattempt otherwise
        let worklet = match AudioWorkletNode::new(&ctx, "cpal-worklet") {
            Ok(w) => {
                Self::send_buffer(&w, &buffer)?;

                Arc::new(Mutex::new(Some(w)))
            }
            Err(_) => {
                let w_arc = Arc::new(Mutex::new(None));
                let dst_fut = dst_fut.clone();
                let ctx_worklet = ctx
                    .audio_worklet()
                    .map_err(map_js_err::<BuildStreamError>)?;

                // load module from included js.file
                let module_url = format!(
                    "data:application/javascript,{}",
                    encode_uri_component(AUDIO_WORKLET)
                );
                let promise = ctx_worklet
                    .add_module(module_url.as_str())
                    .map_err(map_js_err::<BuildStreamError>)?;

                let fut_w_arc = w_arc.clone();
                let fut_ctx = ctx.clone();
                let fut_buffer = buffer.clone();
                spawn_local(async move {
                    match JsFuture::from(promise).await {
                        Ok(_) => {
                            let mut opt_mtx = fut_w_arc.lock().unwrap();

                            // attempt creating the node or fail
                            let node = AudioWorkletNode::new(&fut_ctx, "cpal-worklet").unwrap();
                            Self::send_buffer(&node, &fut_buffer).expect("send buffer to worklet");

                            // connect to destinations if any were stored
                            let mut dst_mtx = dst_fut.lock().unwrap();
                            for dst in dst_mtx.drain(0..) {
                                _ = node.connect_with_audio_node(&dst).unwrap();
                            }

                            _ = opt_mtx.insert(node);
                        }
                        Err(e) => {
                            let err = map_js_err::<BuildStreamError>(e);
                            panic!("{}", err)
                        }
                    }
                });

                w_arc
            }
        };

        // load worker from the included js.file
        let script_url = format!(
            "data:application/javascript,{}",
            encode_uri_component(SCHEDULING_WORKER)
        );
        let waiter =
            Arc::new(Worker::new(script_url.as_str()).map_err(map_js_err::<BuildStreamError>)?);

        Ok(Self {
            worklet,
            waiter,
            buffer,
            view,
            ints,
            floats,
            ctx,
            dst_fut,
            input_cb: None,
            output_cb: None,
        })
    }

    fn send_buffer(
        node: &AudioWorkletNode,
        buffer: &SharedArrayBuffer,
    ) -> Result<(), BuildStreamError> {
        let message = Object::new();
        Reflect::set(&message, &"type".into(), &"buffer".into())
            .map_err(map_js_err::<BuildStreamError>)?;
        Reflect::set(&message, &"buffer".into(), buffer).map_err(map_js_err::<BuildStreamError>)?;
        let port = node.port().map_err(map_js_err::<BuildStreamError>)?;
        port.post_message(&message.into())
            .map_err(map_js_err::<BuildStreamError>)?;

        Ok(())
    }

    pub fn register_output_callback(
        &mut self,
        mut cb: Box<dyn FnMut(&Float32Array)>,
    ) -> Result<(), BuildStreamError> {
        let floats = self.floats.clone();
        let view = self.view.clone();
        let ints = self.ints.clone();

        let listener = Closure::wrap(Box::new(move |msg: JsValue| {
            let message = MessageEvent::from(msg);

            if let Ok(t) = Reflect::get(&message.data(), &"type".into()) {
                if Some("output_data".to_string()) == JsString::from(t).as_string() {
                    log::debug!("output callback");
                    // update the values from callback
                    cb(&floats);

                    // store new values from the data view
                    for i in 0..floats.length() {
                        let int = view.get_int32((BYTE_SIZE * i).try_into().unwrap());
                        _ = Atomics::store(&ints, i, int).expect("store");
                    }

                    // tick the phase
                    let p: u32 = floats.length();
                    _ = Atomics::store(&ints, p, BridgePhase::Output as i32).expect("store");
                    Atomics::notify(&ints, p).unwrap();
                }
            }
        }) as Box<dyn FnMut(JsValue)>);

        self.waiter
            .add_event_listener_with_callback("message", listener.as_ref().unchecked_ref())
            .map_err(map_js_err::<BuildStreamError>)?;

        _ = self.output_cb.insert(listener);

        Ok(())
    }

    pub fn register_input_callback(
        &mut self,
        mut cb: Box<dyn FnMut(&Float32Array)>,
    ) -> Result<(), BuildStreamError> {
        let floats = self.floats.clone();
        let view = self.view.clone();
        let ints = self.ints.clone();
        let listener = Closure::wrap(Box::new(move |msg: JsValue| {
            let message = MessageEvent::from(msg);

            if let Ok(t) = Reflect::get(&message.data(), &"type".into()) {
                if Some("input_data".to_string()) == JsString::from(t).as_string() {
                    log::debug!("input callback");
                    // load the values on the data view
                    for i in 0..floats.length() {
                        let int = Atomics::load(&ints, i).expect("value");
                        view.set_int32((BYTE_SIZE * i).try_into().unwrap(), int);
                    }

                    // call the input callback with updated floats
                    cb(&floats);

                    // tick the phase
                    let p: u32 = floats.length();
                    _ = Atomics::store(&ints, p, BridgePhase::Input as i32).expect("store");
                    Atomics::notify(&ints, p).unwrap();
                }
            }
        }) as Box<dyn FnMut(JsValue)>);

        self.waiter
            .add_event_listener_with_callback("message", listener.as_ref().unchecked_ref())
            .map_err(map_js_err::<BuildStreamError>)?;

        _ = self.input_cb.insert(listener);

        Ok(())
    }

    pub fn cancel_next_tick(&self) -> Result<(), PauseStreamError> {
        let message = Object::new();
        Reflect::set(&message, &"type".into(), &"cancel_tick".into())
            .map_err(map_js_err::<PauseStreamError>)?;

        self.waiter
            .post_message(&message)
            .map_err(map_js_err::<PauseStreamError>)
    }

    pub fn schedule_next_tick(&self) -> Result<(), PlayStreamError> {
        let message = Object::new();
        Reflect::set(&message, &"type".into(), &"schedule_tick".into())
            .map_err(map_js_err::<PlayStreamError>)?;
        Reflect::set(&message, &"buffer".into(), &self.buffer)
            .map_err(map_js_err::<PlayStreamError>)?;
        Reflect::set(&message, &"input".into(), &self.input_cb.is_some().into())
            .map_err(map_js_err::<PlayStreamError>)?;
        Reflect::set(&message, &"output".into(), &self.output_cb.is_some().into())
            .map_err(map_js_err::<PlayStreamError>)?;

        self.waiter
            .post_message(&message)
            .map_err(map_js_err::<PlayStreamError>)
    }

    /// connect with AudioWorkletNode if it is already initialized
    /// store the destination node and connect later otherwise
    pub fn connect_with_audio_node(&mut self, dst: Arc<AudioNode>) -> Result<(), BuildStreamError> {
        if let Some(w) = self.worklet.lock().unwrap().as_ref() {
            w.connect_with_audio_node(&dst)
                .map_err(map_js_err::<BuildStreamError>)?;
            Ok(())
        } else {
            _ = self.dst_fut.lock().unwrap().push(dst.clone());
            Ok(())
        }
    }
}
