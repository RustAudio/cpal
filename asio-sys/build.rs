extern crate bindgen;
extern crate cc;
extern crate walkdir;

use std::env;
use std::path::PathBuf;
use walkdir::WalkDir;

const CPAL_ASIO_DIR: &'static str = "CPAL_ASIO_DIR";

const ASIO_HEADER: &'static str = "asio.h";
const ASIO_SYS_HEADER: &'static str = "asiosys.h";
const ASIO_DRIVERS_HEADER: &'static str = "asiodrivers.h";

fn main() {
    // If ASIO directory isn't set silently return early
    let cpal_asio_dir_var = match env::var(CPAL_ASIO_DIR) {
        Err(_) => return,
        Ok(var) => var,
    };

    // Asio directory
    let cpal_asio_dir = PathBuf::from(cpal_asio_dir_var);

    // Directory where bindings and library are created
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("bad path"));

    // Check if library exists
    // if it doesn't create it
    let mut lib_path = out_dir.clone();
    lib_path.push("libasio.a");
    if !lib_path.exists() {
        create_lib(&cpal_asio_dir);
    }

    // Print out links to needed libraries
    println!("cargo:rustc-link-lib=dylib=ole32");
    println!("cargo:rustc-link-lib=dylib=User32");
    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=asio");

    // Check if bindings exist
    // if they dont create them
    let mut binding_path = out_dir.clone();
    binding_path.push("asio_bindings.rs");
    if !binding_path.exists() {
        create_bindings(&cpal_asio_dir);
    }
}

fn create_lib(cpal_asio_dir: &PathBuf) {
    let mut cpp_paths: Vec<PathBuf> = Vec::new();
    let mut host_dir = cpal_asio_dir.clone();
    let mut pc_dir = cpal_asio_dir.clone();
    let mut common_dir = cpal_asio_dir.clone();
    host_dir.push("host");
    common_dir.push("common");
    pc_dir.push("host/pc");

    // Gathers cpp files from directories
    let walk_a_dir = |dir_to_walk, paths: &mut Vec<PathBuf>| {
        for entry in WalkDir::new(&dir_to_walk).max_depth(1) {
            let entry = match entry {
                Err(_) => continue,
                Ok(entry) => entry,
            };
            match entry.path().extension().and_then(|s| s.to_str()) {
                None => continue,
                Some("cpp") => {
                    // Skip macos bindings
                    if entry.path().file_name().unwrap().to_str() == Some("asiodrvr.cpp") {
                        continue;
                    }
                    paths.push(entry.path().to_path_buf())
                }
                Some(_) => continue,
            };
        }
    };

    // Get all cpp files for building SDK library
    walk_a_dir(host_dir, &mut cpp_paths);
    walk_a_dir(pc_dir, &mut cpp_paths);
    walk_a_dir(common_dir, &mut cpp_paths);

    // build the asio lib
    cc::Build::new()
        .include(format!("{}/{}", cpal_asio_dir.display(), "host"))
        .include(format!("{}/{}", cpal_asio_dir.display(), "common"))
        .include(format!("{}/{}", cpal_asio_dir.display(), "host/pc"))
        .include("asio-link/helpers.hpp")
        .file("asio-link/helpers.cpp")
        .files(cpp_paths)
        .cpp(true)
        .compile("libasio.a");
}

fn create_bindings(cpal_asio_dir: &PathBuf) {
    let mut asio_header = None;
    let mut asio_sys_header = None;
    let mut asio_drivers_header = None;

    // Recursively walk given cpal dir to find required headers
    for entry in WalkDir::new(&cpal_asio_dir) {
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
        .header("asio-link/helpers.hpp")
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++14")
        .clang_arg( format!("-I{}/{}", cpal_asio_dir.display(), "host/pc") )
        .clang_arg( format!("-I{}/{}", cpal_asio_dir.display(), "host") )
        .clang_arg( format!("-I{}/{}", cpal_asio_dir.display(), "common") )
        // Need to whitelist to avoid binding tp c++ std::*
        .whitelist_type("AsioDrivers")
        .whitelist_type("AsioDriver")
        .whitelist_type("ASIOTime")
        .whitelist_type("ASIODriverInfo")
        .whitelist_type("ASIOBufferInfo")
        .whitelist_type("ASIOCallbacks")
        .whitelist_type("ASIOSampleType")
        .whitelist_type("ASIOChannelInfo")
        .whitelist_function("destruct_AsioDrivers")
        .whitelist_function("ASIOGetChannels")
        .whitelist_function("ASIOGetChannelInfo")
        .whitelist_function("ASIOGetBufferSize")
        .whitelist_function("get_sample_rate")
        .whitelist_function("ASIOInit")
        .whitelist_function("ASIOCreateBuffers")
        .whitelist_function("ASIOStart")
        .whitelist_function("ASIOStop")
        .whitelist_function("ASIODisposeBuffers")
        .whitelist_function("ASIOExit")
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
