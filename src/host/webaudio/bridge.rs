use web_sys::{AudioContext, AudioNode};
use js_sys::Function;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/host/webaudio/bridge.js")]
extern "C" {
  
  #[wasm_bindgen(extends = AudioNode)]
  pub type CpalBridge;

  #[wasm_bindgen(constructor)]
  pub fn new(ctx: &AudioContext, channels: u16, frames: u32) -> CpalBridge;

  #[wasm_bindgen(method, js_name="registerInputCallback")]
  pub fn register_input_callback(this: &CpalBridge, cb: &Function);
  
  #[wasm_bindgen(method, js_name="registerOutputCallback")]
  pub fn register_output_callback(this: &CpalBridge, cb: &Function, interval: usize);

  #[wasm_bindgen(method)]
  pub fn stop(this: &CpalBridge);
  
  #[wasm_bindgen(method)]
  pub fn resume(this: &CpalBridge);
}