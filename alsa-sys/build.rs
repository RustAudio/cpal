extern crate pkg_config;

fn main() {
    pkg_config::find_library("alsa")
        .expect("It seems you have no alsa dev package.\
On Ubuntu based systems you can install it with 'sudo apt install libasound2-dev'.");
}
