// Exercise the WebAssembly callback boundary on native hosts so aborts can be
// isolated and checked in child processes.
#[cfg(not(target_arch = "wasm32"))]
#[path = "../src/host/audioworklet/aborting.rs"]
mod aborting;
