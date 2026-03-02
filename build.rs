use std::env;

const CPAL_ASIO_DIR: &str = "CPAL_ASIO_DIR";

fn main() {
    println!("cargo:rerun-if-env-changed={CPAL_ASIO_DIR}");
    if env::var(CPAL_ASIO_DIR).is_ok() {
        println!("cargo:rustc-cfg=asio");
    }
}
