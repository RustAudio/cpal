mod implementation;
pub use implementation::PipewireClient;
mod connection_string;
mod handlers;
mod api;
mod channel;

#[cfg(test)]
pub(super) use api::CoreApi;

#[cfg(test)]
#[path = "./implementation_test.rs"]
mod implementation_test;

#[cfg(test)]
#[path = "./channel_test.rs"]
mod channel_test;