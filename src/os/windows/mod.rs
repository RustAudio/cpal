pub enum Backend {
    Wasapi,
    Asio,
}

// TODO This needs to be set once at run time
// by the cpal user
static backend: Backend = Backend::Asio;

pub fn which_backend() -> &'static Backend {
    &backend
}
