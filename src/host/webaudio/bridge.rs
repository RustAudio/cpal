use std::sync::{Arc, Mutex};

use js_sys::{
    encode_uri_component, Atomics, DataView, Float32Array, Int32Array, Object, Promise, Reflect,
    SharedArrayBuffer,
};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AbortController, AbortSignal, AudioContext, AudioNode, AudioWorkletNode, MessageEvent,
};

use wasm_bindgen::prelude::*;

use crate::{BackendSpecificError, BuildStreamError, PauseStreamError, PlayStreamError};

use super::map_js_err;

// TODO: minify etc
const AUDIO_WORKLET: &str = include_str!("worklet.js");

// Float32Array.BYTES_PER_ELEMENT = 4
// Int32Array.BYTES_PER_ELEMENT = 4
const BYTE_SIZE: u32 = 4;

#[derive(PartialEq, Eq, Debug)]
pub(crate) enum BridgePhase {
    /// audio worklet done processing buffer
    WorkletDone = 0,
    /// main completed calls to its input or output callbacks
    MainDone = 1,
}

pub(crate) struct WebAudioBridge {
    /// plays the shared buffer
    worklet: Arc<Mutex<Option<AudioWorkletNode>>>,
    /// once module is added these are connected to worklet
    dst_fut: Arc<Mutex<Vec<Arc<AudioNode>>>>,
    /// once module is added register message listener
    message_fut: Arc<Mutex<bool>>,
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
    /// store callback
    callback: Arc<Mutex<Option<Box<dyn FnMut()>>>>,
    /// store closure for message_based fallback
    on_message: Arc<Closure<dyn FnMut(JsValue)>>,
    /// signal to abort next tick
    abort: Arc<Mutex<AbortController>>,
}

impl WebAudioBridge {
    pub fn new(
        ctx: Arc<AudioContext>,
        channels: u16,
        frames: u32,
        input: bool,
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

        let callback: Arc<Mutex<Option<Box<dyn FnMut()>>>> = Arc::new(Mutex::new(None));

        let on_message_cb = callback.clone();
        let on_message = Arc::new(Closure::wrap(Box::new(move |e: JsValue| {
            let ev = MessageEvent::from(e);
            let t = Reflect::get(&ev.data(), &"type".into()).ok();
            if Some("worklet_done".to_string()) == t.map(|v| v.as_string()).flatten() {
                let mut cb_mtx = on_message_cb.lock().unwrap();
                let cb = cb_mtx.as_mut().unwrap();
                cb()
            }
        }) as Box<dyn FnMut(JsValue)>));

        let abort = Arc::new(Mutex::new(
            AbortController::new().map_err(map_js_err::<BuildStreamError>)?,
        ));

        let message_fut = Arc::new(Mutex::new(false));

        // try creating new worklet or add module and reattempt otherwise
        let worklet = match AudioWorkletNode::new(&ctx, "cpal-worklet") {
            Ok(w) => {
                Self::send_buffer(&w, &buffer, input)?;

                Arc::new(Mutex::new(Some(w)))
            }
            Err(_) => {
                let w_arc = Arc::new(Mutex::new(None));
                let dst_fut = dst_fut.clone();
                let ctx_worklet = ctx
                    .audio_worklet()
                    .map_err(map_js_err::<BuildStreamError>)?;
                let message_fut = message_fut.clone();
                let on_message = on_message.clone();

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
                            Self::send_buffer(&node, &fut_buffer, input)
                                .expect("send buffer to worklet");

                            // connect to destinations if any were stored
                            let mut dst_mtx = dst_fut.lock().unwrap();
                            for dst in dst_mtx.drain(0..) {
                                _ = node.connect_with_audio_node(&dst).unwrap();
                            }

                            // set message listener if necessary
                            if *message_fut.lock().unwrap() {
                                let port = node.port().unwrap();
                                port.set_onmessage(Some(
                                    Closure::as_ref(&on_message).unchecked_ref(),
                                ));
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

        Ok(Self {
            worklet,
            buffer,
            view,
            ints,
            floats,
            ctx,
            dst_fut,
            callback,
            on_message,
            abort,
            message_fut,
        })
    }

    fn send_buffer(
        node: &AudioWorkletNode,
        buffer: &SharedArrayBuffer,
        input: bool,
    ) -> Result<(), BuildStreamError> {
        let message = Object::new();
        Reflect::set(&message, &"type".into(), &"buffer".into())
            .map_err(map_js_err::<BuildStreamError>)?;
        Reflect::set(&message, &"buffer".into(), buffer).map_err(map_js_err::<BuildStreamError>)?;
        Reflect::set(&message, &"isInput".into(), &input.into())
            .map_err(map_js_err::<BuildStreamError>)?;
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
        let callback = Box::new(move || {
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
            _ = Atomics::store(&ints, p, BridgePhase::MainDone as i32).expect("store");
            Atomics::notify(&ints, p).unwrap();
        }) as Box<dyn FnMut()>;

        if let Some(_) = self.callback.lock().unwrap().replace(callback) {
            Err(BackendSpecificError {
                description: "callback already registered".to_string(),
            }
            .into())
        } else {
            Ok(())
        }
    }

    pub fn register_input_callback(
        &mut self,
        mut cb: Box<dyn FnMut(&Float32Array)>,
    ) -> Result<(), BuildStreamError> {
        let floats = self.floats.clone();
        let view = self.view.clone();
        let ints = self.ints.clone();
        let callback = Box::new(move || {
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
            _ = Atomics::store(&ints, p, BridgePhase::MainDone as i32).expect("store");
            Atomics::notify(&ints, p).unwrap();
        }) as Box<dyn FnMut()>;

        if let Some(_) = self.callback.lock().unwrap().replace(callback) {
            Err(BackendSpecificError {
                description: "callback already registered".to_string(),
            }
            .into())
        } else {
            Ok(())
        }
    }

    pub fn cancel_next_tick(&self) -> Result<(), PauseStreamError> {
        let mut abort_mtx = self.abort.lock().unwrap();
        abort_mtx.abort();
        *abort_mtx = AbortController::new().map_err(map_js_err::<PauseStreamError>)?;

        if let Some(node) = self.worklet.lock().unwrap().as_ref() {
            let port = node.port().map_err(map_js_err::<PauseStreamError>)?;
            port.set_onmessage(None);
            Ok(())
        } else {
            log::error!("DeviceNotAvailable yet in cancel_next_tick");
            Err(PauseStreamError::DeviceNotAvailable)
        }
    }

    pub fn schedule_next_tick(&self) -> Result<(), PlayStreamError> {
        let ints = self.ints.clone();
        let floats = self.floats.clone();
        let callback = self.callback.clone();
        let signal = self.abort.lock().unwrap().signal();
        match Self::schedule_next(ints, floats, callback.clone(), signal) {
            Ok(_) => Ok(()),
            Err(_) => {
                // https://bugzilla.mozilla.org/show_bug.cgi?id=1467846
                // fallback to events
                if let Some(node) = self.worklet.lock().unwrap().as_ref() {
                    let port = node.port().map_err(map_js_err::<PlayStreamError>)?;
                    let on_message = self.on_message.clone();
                    port.set_onmessage(Some(Closure::as_ref(&on_message).unchecked_ref()));
                } else {
                    let mut set_listener = self.message_fut.lock().unwrap();
                    *set_listener = true;
                }
                Ok(())
            }
        }
    }

    fn schedule_next(
        ints: Arc<Int32Array>,
        floats: Arc<Float32Array>,
        callback: Arc<Mutex<Option<Box<dyn FnMut()>>>>,
        signal: AbortSignal,
    ) -> Result<(), PlayStreamError> {
        let obj = Atomics::wait_async(&ints, floats.length(), BridgePhase::MainDone as i32)
            .map_err(map_js_err::<PlayStreamError>)?;

        if signal.aborted() {
            return Ok(());
        }

        if Reflect::get(&obj, &"async".into())
            .map(|v| v.as_bool().unwrap_or_default())
            .ok()
            .unwrap_or_default()
        {
            let value =
                Reflect::get(&obj, &"value".into()).map_err(map_js_err::<PlayStreamError>)?;
            let promise = Promise::from(value);
            let cb_mtx = callback.clone();
            spawn_local(async move {
                JsFuture::from(promise).await.unwrap();
                {
                    let mut cb_opt = cb_mtx.lock().unwrap();
                    let cb = cb_opt.as_mut().unwrap();
                    cb();
                }

                Self::schedule_next(ints, floats, cb_mtx, signal).unwrap();
            });
            Ok(())
        } else if Some("not-equal".to_string())
            == Reflect::get(&obj, &"value".into())
                .map(|v| v.as_string())
                .ok()
                .flatten()
        {
            if let Some(cb) = callback.lock().unwrap().as_mut() {
                cb();
            } else {
                return Err(BackendSpecificError {
                    description: "no callback".to_string(),
                }
                .into());
            }
            Self::schedule_next(ints, floats, callback, signal)
        } else {
            log::error!("DeviceNotAvailable yet in Self::schedule_next");
            Err(PlayStreamError::DeviceNotAvailable)
        }
    }

    /// connect with AudioWorkletNode if it's already initialized
    /// otherwise store the destination node and connect later
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
