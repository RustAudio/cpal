pub enum Backend {
    Wasapi,
    Asio,
}

static backend: Backend = Backend::Asio;

pub fn which_backend() -> &'static Backend {
    &backend
}
