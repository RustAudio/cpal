/// This allows you to choose either Wasapi or ASIO 
/// as your back end. Wasapi is the default.
/// The CPAL_ASIO_DIR must be set to the ASIO SDK
/// directory for use_asio_backend to be available. 
use std::sync::Mutex;

#[derive(Clone)]
pub enum BackEnd {
    Wasapi,
    Asio,
}

lazy_static! {
    static ref BACK_END: Mutex<BackEnd> = Mutex::new(BackEnd::Wasapi);
}

/// See which beackend is currently set.
pub fn which_backend() -> BackEnd {
    (*BACK_END.lock().unwrap()).clone()
}

#[cfg(asio)]
/// Choose ASIO as the backend 
pub fn use_asio_backend() -> Result<(), BackEndError> {
    *BACK_END.lock().unwrap() = BackEnd::Asio;
    Ok(())
}

/// Choose Wasapi as the backend 
pub fn use_wasapi_backend() -> Result<(), BackEndError> {
    *BACK_END.lock().unwrap() = BackEnd::Wasapi;
    Ok(())
}

#[derive(Debug)]
pub struct BackEndError;