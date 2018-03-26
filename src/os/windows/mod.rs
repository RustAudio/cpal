pub enum Backend {
    Wasapi,
    Asio,
}

static mut backend: Backend = Backend::Wasapi;

pub fn which_backend() -> Backend {
    backend
}
