use std::env;
const CPAL_ASIO_DIR: &str = "CPAL_ASIO_DIR";

fn main() {
    println!("cargo:rerun-if-env-changed={}", CPAL_ASIO_DIR);

    // This avoids runtime errors on Android targets.
    if std::env::var("TARGET").unwrap().contains("android") {
        println!("cargo:rustc-link-lib=c++_shared");
    }

    // If ASIO directory isn't set silently return early
    // otherwise set the asio config flag
    match env::var(CPAL_ASIO_DIR) {
        Err(_) => {}
        Ok(_) => println!("cargo:rustc-cfg=asio"),
    };
}
