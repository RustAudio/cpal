extern crate bindgen;
extern crate walkdir;

use std::env;
use std::path::PathBuf;
use walkdir::WalkDir;

const CPAL_ASIO_DIR: &'static str = "CPAL_ASIO_DIR";

const ASIO_HEADER: &'static str = "asio.h";
const ASIO_SYS_HEADER: &'static str = "asiosys.h";
const ASIO_DRIVERS_HEADER: &'static str = "asiodrivers.h";

fn main() {
    println!("cargo:rust-link-lib=static=asio");
    // If ASIO directory isn't set silently return early
    let cpal_asio_dir_var = match env::var(CPAL_ASIO_DIR) {
        Err(_) => { 
            //panic!("didn't load ");
            return;
        }
        Ok(var) => var,
    };

    let cpal_asio_dir = PathBuf::from(cpal_asio_dir_var);

    let mut asio_header = None;
    let mut asio_sys_header = None;
    let mut asio_drivers_header = None;
    // Recursively walk given cpal dir to find required headers
    for entry in WalkDir::new(&cpal_asio_dir){
        let entry = match entry {
            Err(_) => continue,
            Ok(entry) => entry,
        };
        let file_name = match entry.path().file_name().and_then(|s| s.to_str()) {
            None => continue,
            Some(file_name) => file_name,
        };

        match file_name {
           ASIO_HEADER => asio_header = Some(entry.path().to_path_buf()), 
           ASIO_SYS_HEADER => asio_sys_header = Some(entry.path().to_path_buf()), 
           ASIO_DRIVERS_HEADER => asio_drivers_header = Some(entry.path().to_path_buf()), 
           _ => (),
        }
    }

    macro_rules! header_or_panic {
        ($opt_header:expr, $FILE_NAME:expr) => {
            match $opt_header.as_ref() {
                None => {
                    panic!("Could not find {} in {}: {}", $FILE_NAME, CPAL_ASIO_DIR, cpal_asio_dir.display());
                },
                Some(path) => path.to_str().expect("Could not convert path to str"),
            }
        };
    }

    // Only continue if found all headers that we need
    let asio_header = header_or_panic!(asio_header, ASIO_HEADER);
    let asio_sys_header = header_or_panic!(asio_sys_header, ASIO_SYS_HEADER);
    let asio_drivers_header = header_or_panic!(asio_drivers_header, ASIO_DRIVERS_HEADER);

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header(asio_header)
        .header(asio_sys_header)
        .header(asio_drivers_header)
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++14")
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").expect("bad path"));
    //panic!("path: {}", out_path.display());
    bindings
        .write_to_file(out_path.join("asio_bindings.rs"))
        .expect("Couldn't write bindings!");
}
