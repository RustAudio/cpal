mod implementation;
pub use implementation::PipewireClient;
mod connection_string;
mod handlers;
mod api;
pub(super) use api::CoreApi;

#[cfg(test)]
#[path = "./implementation_test.rs"]
mod implementation_test;