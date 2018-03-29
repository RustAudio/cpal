pub enum Backend {
    Wasapi,
    Asio,
}

static backend: Backend = Backend::Wasapi;

pub fn which_backend() -> &'static Backend {
    &backend
}
