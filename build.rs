use std::env;

const CPAL_ASIO_DIR: &'static str = "CPAL_ASIO_DIR";

fn main() {
    // If ASIO directory isn't set silently return early
    // otherwise set the asio config flag
    match env::var(CPAL_ASIO_DIR) {
        Err(_) => return,
        Ok(_) => println!("cargo:rustc-cfg=asio"),
    };
}
