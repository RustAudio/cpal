use std::sync::Mutex;

#[derive(Clone)]
pub enum BackEnd {
    Wasapi,
    Asio,
}

//static BACKEND: BackEnd = BackEnd::Asio;

lazy_static! {
    static ref BACK_END: Mutex<BackEnd> = Mutex::new(BackEnd::Wasapi);
}

pub fn which_backend() -> BackEnd {
    (*BACK_END.lock().unwrap()).clone()
}

#[cfg(asio)]
pub fn use_asio_backend() -> Result<(), BackEndError> {
    *BACK_END.lock().unwrap() = BackEnd::Asio;
    Ok(())
}

pub fn use_wasapi_backend() -> Result<(), BackEndError> {
    *BACK_END.lock().unwrap() = BackEnd::Wasapi;
    Ok(())
}

#[derive(Debug)]
pub struct BackEndError;