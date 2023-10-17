extern crate bindgen;
extern crate cc;
extern crate walkdir;

use std::env;
use std::path::PathBuf;
use std::process::Command;
use walkdir::WalkDir;

const CPAL_ASIO_DIR: &str = "CPAL_ASIO_DIR";
const ASIO_SDK_URL: &str = "https://www.steinberg.net/asiosdk";

const ASIO_HEADER: &str = "asio.h";
const ASIO_SYS_HEADER: &str = "asiosys.h";
const ASIO_DRIVERS_HEADER: &str = "asiodrivers.h";

fn main() {
    println!("cargo:rerun-if-env-changed={}", CPAL_ASIO_DIR);

    // ASIO SDK directory
    let cpal_asio_dir = get_asio_dir();
    println!("cargo:rerun-if-changed={}", cpal_asio_dir.display());

    // Directory where bindings and library are created
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("bad path"));

    let mut vc_vars_invoked = false;

    // Check if library exists,
    // if it doesn't create it
    let mut lib_path = out_dir.clone();
    lib_path.push("libasio.a");
    if !lib_path.exists() {
        // Set env vars from vcvarsall.bat before attempting to build
        invoke_vcvars();
        vc_vars_invoked = true;
        create_lib(&cpal_asio_dir);
    }

    // Print out links to needed libraries
    println!("cargo:rustc-link-lib=dylib=ole32");
    println!("cargo:rustc-link-lib=dylib=User32");
    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=asio");
    println!("cargo:rustc-cfg=asio");

    // Check if bindings exist
    // If they don't create them
    let mut binding_path = out_dir.clone();
    binding_path.push("asio_bindings.rs");
    if !binding_path.exists() {
        // Set env vars from vcvarsall.bat before attempting to build if not already done
        if !vc_vars_invoked {
            invoke_vcvars();
        }
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
                Err(e) => {
                    println!("error: {}", e);
                    continue;
                }
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
                    panic!(
                        "Could not find {} in {}: {}",
                        $FILE_NAME,
                        CPAL_ASIO_DIR,
                        cpal_asio_dir.display()
                    );
                }
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
        .clang_arg(format!("-I{}/{}", cpal_asio_dir.display(), "host/pc"))
        .clang_arg(format!("-I{}/{}", cpal_asio_dir.display(), "host"))
        .clang_arg(format!("-I{}/{}", cpal_asio_dir.display(), "common"))
        // Need to whitelist to avoid binding tp c++ std::*
        .allowlist_type("AsioDrivers")
        .allowlist_type("AsioDriver")
        .allowlist_type("ASIOTime")
        .allowlist_type("ASIOTimeInfo")
        .allowlist_type("ASIODriverInfo")
        .allowlist_type("ASIOBufferInfo")
        .allowlist_type("ASIOCallbacks")
        .allowlist_type("ASIOSamples")
        .allowlist_type("ASIOSampleType")
        .allowlist_type("ASIOSampleRate")
        .allowlist_type("ASIOChannelInfo")
        .allowlist_type("AsioTimeInfoFlags")
        .allowlist_type("ASIOTimeCodeFlags")
        .allowlist_function("ASIOGetChannels")
        .allowlist_function("ASIOGetChannelInfo")
        .allowlist_function("ASIOGetBufferSize")
        .allowlist_function("ASIOGetSamplePosition")
        .allowlist_function("get_sample_rate")
        .allowlist_function("set_sample_rate")
        .allowlist_function("can_sample_rate")
        .allowlist_function("ASIOInit")
        .allowlist_function("ASIOCreateBuffers")
        .allowlist_function("ASIOStart")
        .allowlist_function("ASIOStop")
        .allowlist_function("ASIODisposeBuffers")
        .allowlist_function("ASIOExit")
        .allowlist_function("load_asio_driver")
        .allowlist_function("remove_current_driver")
        .allowlist_function("get_driver_names")
        .bitfield_enum("AsioTimeInfoFlags")
        .bitfield_enum("ASIOTimeCodeFlags")
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

fn get_asio_dir() -> PathBuf {
    // Check if CPAL_ASIO_DIR env var is set
    if let Ok(path) = env::var(CPAL_ASIO_DIR) {
        println!("CPAL_ASIO_DIR is set at {}", path);
        return PathBuf::from(path);
    }

    // If not set, check temp directory for ASIO SDK, maybe it is previously downloaded
    let temp_dir = env::temp_dir();
    let asio_dir = temp_dir.join("asio_sdk");
    if asio_dir.exists() {
        println!("CPAL_ASIO_DIR is set at {}", asio_dir.display());
        return asio_dir;
    }

    // If not found, download ASIO SDK using PowerShell's Invoke-WebRequest
    println!("CPAL_ASIO_DIR is not set or contents are cached downloading from {}", ASIO_SDK_URL);

    let asio_zip_path = temp_dir.join("asio_sdk.zip");
    let status = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            &format!(
                "Invoke-WebRequest -Uri {} -OutFile {}",
                ASIO_SDK_URL,
                asio_zip_path.display()
            ),
        ])
        .status()
        .expect("Failed to execute PowerShell command");

    if !status.success() {
        panic!("Failed to download ASIO SDK");
    }
    println!("Downloaded ASIO SDK successfully");

    // Unzip using PowerShell's Expand-Archive
    println!("Extracting ASIO SDK..");
    let status = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            &format!(
                "Expand-Archive -Path {} -DestinationPath {} -Force",
                asio_zip_path.display(),
                temp_dir.display()
            ),
        ])
        .status()
        .expect("Failed to execute PowerShell command for extracting ASIO SDK");

    if !status.success() {
        panic!("Failed to extract ASIO SDK");
    }

    // Move the contents of the inner directory to asio_dir
    for entry in walkdir::WalkDir::new(&temp_dir).min_depth(1).max_depth(1) {
        let entry = entry.unwrap();
        if entry.file_type().is_dir() && entry.file_name().to_string_lossy().starts_with("asio") {
            std::fs::rename(entry.path(), &asio_dir).expect("Failed to rename directory");
            break;
        }
    }
    println!("CPAL_ASIO_DIR is set at {}", asio_dir.display());
    asio_dir
}

fn invoke_vcvars() {
    println!("Invoking vcvarsall.bat..");
    println!("Determining system architecture..");

    // Determine the system architecture to be used as an argument to vcvarsall.bat
    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        panic!("Unsupported architecture");
    };

    println!("Architecture detected as {}.", arch);

    // Define search paths for vcvarsall.bat based on architecture
    let paths = if arch == "amd64" {
        vec![
            "C:\\Program Files (x86)\\Microsoft Visual Studio\\",
            "C:\\Program Files\\Microsoft Visual Studio\\",
        ]
    } else {
        vec!["C:\\Program Files\\Microsoft Visual Studio\\"]
    };

    // Search for vcvarsall.bat using walkdir
    println!("Searching for vcvarsall.bat in {paths:?}");
    for path in paths.iter() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| !e.file_type().is_dir())
        {
            if entry.path().ends_with("vcvarsall.bat") {
                println!(
                    "Found vcvarsall.bat at {}. Initializing environment..",
                    entry.path().display()
                );

                // Invoke vcvarsall.bat
                let output = Command::new("cmd")
                    .args(&["/c", entry.path().to_str().unwrap(), &arch, "&&", "set"])
                    .output()
                    .expect("Failed to execute command");

                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    // Filters the output of vcvarsall.bat to only include lines of the form "VARNAME=VALUE"
                    let parts: Vec<&str> = line.splitn(2, '=').collect();
                    if parts.len() == 2 {
                        env::set_var(parts[0], parts[1]);
                        println!("{}={}", parts[0], parts[1]);
                    }
                }
                return;
            }
        }
    }

    panic!(
        "Could not find vcvarsall.bat. Please install the latest version of Visual Studio."
    );
}
