extern crate bindgen;
extern crate cc;
extern crate parse_cfg;
extern crate walkdir;

use parse_cfg::*;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const CPAL_ASIO_DIR: &str = "CPAL_ASIO_DIR";
const ASIO_SDK_URL: &str = "https://www.steinberg.net/asiosdk";

const ASIO_HEADER: &str = "asio.h";
const ASIO_SYS_HEADER: &str = "asiosys.h";
const ASIO_DRIVERS_HEADER: &str = "asiodrivers.h";

/// Checks if the host OS is Windows
fn host_os_is_windows() -> bool {
    std::env::consts::OS == "windows"
}

/// Checks if the target env is MSVC
fn is_msvc() -> bool {
    let target: Target = std::env::var("TARGET")
        .expect("Target not set.")
        .parse()
        .expect("Unable to parse target.");

    let target_env = match target {
        Target::Triple { env, .. } => env,
        Target::Cfg(_) => panic!("cfg targets not supported"),
    };

    if let Some(env) = target_env {
        env.contains("msvc")
    } else {
        false
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed={}", CPAL_ASIO_DIR);

    // ASIO SDK directory
    let cpal_asio_dir = get_asio_dir();
    println!("cargo:rerun-if-changed={}", cpal_asio_dir.display());

    // Directory where bindings and library are created
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("bad path"));

    // Check if library exists,
    // if it doesn't create it
    let mut lib_path = out_dir.clone();
    lib_path.push("libasio.a");
    if !lib_path.exists() {
        if is_msvc() {
            invoke_vcvars_if_not_set();
        }
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
        if is_msvc() {
            invoke_vcvars_if_not_set();
        }
        create_bindings(&cpal_asio_dir);
    }
}

fn create_lib(cpal_asio_dir: &Path) {
    let mut cpp_paths: Vec<PathBuf> = Vec::new();
    let mut host_dir = cpal_asio_dir.to_path_buf();
    let mut pc_dir = cpal_asio_dir.to_path_buf();
    let mut common_dir = cpal_asio_dir.to_path_buf();
    host_dir.push("host");
    common_dir.push("common");
    pc_dir.push("host/pc");

    // Gathers cpp files from directories
    let walk_a_dir = |dir_to_walk, paths: &mut Vec<PathBuf>| {
        for entry in WalkDir::new(dir_to_walk).max_depth(1) {
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
    for entry in WalkDir::new(cpal_asio_dir) {
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
        .allowlist_function("ASIOOutputReady")
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

    bindings
        .write_to_file(out_path.join("asio_bindings.rs"))
        .expect("Couldn't write bindings!");
}

/// Gets the ASIO SDK directory
///
/// If the CPAL_ASIO_DIR env var is set, it will use that.
///
/// If not set, it will check the temp directory for the ASIO SDK.
///
/// If not found, it will download the ASIO SDK to the temp directory.
///
/// It will then move the contents of the inner directory to the temp directory.
///
/// It will then return the path to the ASIO SDK directory.
fn get_asio_dir() -> PathBuf {
    // Check if CPAL_ASIO_DIR env var is set
    if let Ok(path) = env::var(CPAL_ASIO_DIR) {
        println!("CPAL_ASIO_DIR is set at {path}");
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
    println!("CPAL_ASIO_DIR is not set or contents are cached downloading from {ASIO_SDK_URL}",);

    download_asio_sdk_to_temp_dir(&temp_dir);

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

/// Downloads the ASIO SDK to the temp directory of the host OS
///
/// It uses powershell's Invoke-WebRequest on Windows and curl on other platforms to download the SDK.
///
/// It then extracts the SDK using powershell's Expand-Archive on Windows and unzip on other platforms.
fn download_asio_sdk_to_temp_dir(temp_dir: &Path) {
    let asio_zip_path = temp_dir.join("asio_sdk.zip");
    if host_os_is_windows() {
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Invoke-WebRequest -Uri {ASIO_SDK_URL} -OutFile {}",
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
            .args([
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
    } else {
        let status = Command::new("sh")
            .arg("-c")
            .arg(&format!(
                "curl -L --fail --output {} {}",
                asio_zip_path.display(),
                "https://www.steinberg.net/asiosdk" // Replace with the actual ASIO SDK URL
            ))
            .status()
            .expect("Failed to execute curl command");

        if !status.success() {
            panic!("Failed to download ASIO SDK");
        }
        println!("Downloaded ASIO SDK successfully");

        // Extract using `unzip`
        println!("Extracting ASIO SDK..");
        let status = Command::new("unzip")
            .args([
                "-o",
                asio_zip_path.to_str().unwrap(),
                "-d",
                temp_dir.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to execute unzip command for extracting ASIO SDK");

        if !status.success() {
            panic!("Failed to extract ASIO SDK");
        }
    }
}

/// Invokes `vcvarsall.bat` to initialize the environment for building with MSVC
///
/// This function is only meant to be called when the host OS is Windows.
fn invoke_vcvars_if_not_set() {
    if vcvars_set() {
        return;
    }
    println!("VCINSTALLDIR is not set. Attempting to invoke vcvarsall.bat..");

    println!("Invoking vcvarsall.bat..");
    println!("Determining system architecture..");

    let arch_arg = determine_vcvarsall_bat_arch_arg();
    println!(
        "Host architecture is detected as {}.",
        std::env::consts::ARCH
    );
    println!("Architecture argument for vcvarsall.bat will be used as: {arch_arg}.");

    let vcvars_all_bat_path = search_vcvars_all_bat();

    println!(
        "Found vcvarsall.bat at {}. Initializing environment..",
        vcvars_all_bat_path.display()
    );

    // Invoke vcvarsall.bat
    let output = Command::new("cmd")
        .args([
            "/c",
            vcvars_all_bat_path.to_str().unwrap(),
            &arch_arg,
            "&&",
            "set",
        ])
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
}

/// Checks if vcvarsall.bat has been invoked
/// Assumes that it is very unlikely that the user would set `VCINSTALLDIR` manually
fn vcvars_set() -> bool {
    env::var("VCINSTALLDIR").is_ok()
}

/// Searches for vcvarsall.bat in the default installation directories
///
/// If it is not found, it will search for it in the Program Files directories
///
/// If it is still not found, it will panic.
fn search_vcvars_all_bat() -> PathBuf {
    if let Some(path) = guess_vcvars_all_bat() {
        return path;
    }

    // Define search paths for vcvarsall.bat based on architecture
    let paths = &[
        // Visual Studio 2022+
        "C:\\Program Files\\Microsoft Visual Studio\\",
        // <= Visual Studio 2019
        "C:\\Program Files (x86)\\Microsoft Visual Studio\\",
    ];

    // Search for vcvarsall.bat using walkdir
    println!("Searching for vcvarsall.bat in {paths:?}");

    let mut found = None;

    for path in paths.iter() {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| !e.file_type().is_dir())
        {
            if entry.path().ends_with("vcvarsall.bat") {
                found.replace(entry.path().to_path_buf());
            }
        }
    }

    match found {
        Some(path) => path,
        None => panic!(
            "Could not find vcvarsall.bat. Please install the latest version of Visual Studio."
        ),
    }
}

/// Guesses the location of vcvarsall.bat by searching it with certain heuristics.
///
/// It is meant to be executed before a top level search over Microsoft Visual Studio directories
/// to ensure faster execution in CI environments.
fn guess_vcvars_all_bat() -> Option<PathBuf> {
    /// Checks if a string is a year
    fn is_year(s: Option<&str>) -> Option<String> {
        let Some(s) = s else {
            return None;
        };

        if s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()) {
            Some(s.to_string())
        } else {
            None
        }
    }

    /// Checks if a string is an edition of Visual Studio
    fn is_edition(s: Option<&str>) -> Option<String> {
        let Some(s) = s else {
            return None;
        };

        let editions = ["Enterprise", "Professional", "Community", "Express"];
        if editions.contains(&s) {
            Some(s.to_string())
        } else {
            None
        }
    }

    /// Constructs a path to vcvarsall.bat based on a base path
    fn construct_path(base: &Path) -> Option<PathBuf> {
        let mut constructed = base.to_path_buf();
        for entry in WalkDir::new(&constructed).max_depth(1) {
            let entry = match entry {
                Err(_) => continue,
                Ok(entry) => entry,
            };
            if let Some(year) = is_year(entry.path().file_name().and_then(|s| s.to_str())) {
                constructed = constructed.join(year);
                for entry in WalkDir::new(&constructed).max_depth(1) {
                    let entry = match entry {
                        Err(_) => continue,
                        Ok(entry) => entry,
                    };
                    if let Some(edition) =
                        is_edition(entry.path().file_name().and_then(|s| s.to_str()))
                    {
                        constructed = constructed
                            .join(edition)
                            .join("VC")
                            .join("Auxiliary")
                            .join("Build")
                            .join("vcvarsall.bat");

                        return Some(constructed);
                    }
                }
            }
        }
        None
    }

    let vs_2022_and_onwards_base = PathBuf::from("C:\\Program Files\\Microsoft Visual Studio\\");
    let vs_2019_and_2017_base = PathBuf::from("C:\\Program Files (x86)\\Microsoft Visual Studio\\");

    construct_path(&vs_2022_and_onwards_base).map_or_else(
        || construct_path(&vs_2019_and_2017_base).map_or_else(|| None, Some),
        Some,
    )
}

/// Determines the right argument to pass to `vcvarsall.bat` based on the host and target architectures.
///
/// Windows on ARM is not supporting 32 bit arm processors.
/// Because of this there is no native or cross compilation is supported for 32 bit arm processors.
fn determine_vcvarsall_bat_arch_arg() -> String {
    let host_architecture = std::env::consts::ARCH;
    let target_architecture = std::env::var("CARGO_CFG_TARGET_ARCH").expect("Target not set.");

    let arch_arg = if target_architecture == "x86_64" {
        if host_architecture == "x86" {
            // Arg for cross compilation from x86 to x64
            "x86_amd64"
        } else if host_architecture == "x86_64" {
            // Arg for native compilation from x64 to x64
            "amd64"
        } else if host_architecture == "aarch64" {
            // Arg for cross compilation from arm64 to amd64
            "arm64_amd64"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else if target_architecture == "x86" {
        if host_architecture == "x86" {
            // Arg for native compilation from x86 to x86
            "x86"
        } else if host_architecture == "x86_64" {
            // Arg for cross compilation from x64 to x86
            "amd64_x86"
        } else if host_architecture == "aarch64" {
            // Arg for cross compilation from arm64 to x86
            "arm64_x86"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else if target_architecture == "arm" {
        if host_architecture == "x86" {
            // Arg for cross compilation from x86 to arm
            "x86_arm"
        } else if host_architecture == "x86_64" {
            // Arg for cross compilation from x64 to arm
            "amd64_arm"
        } else if host_architecture == "aarch64" {
            // Arg for cross compilation from arm64 to arm
            "arm64_arm"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else if target_architecture == "aarch64" {
        if host_architecture == "x86" {
            // Arg for cross compilation from x86 to arm
            "x86_arm64"
        } else if host_architecture == "x86_64" {
            // Arg for cross compilation from x64 to arm
            "amd64_arm64"
        } else if host_architecture == "aarch64" {
            // Arg for native compilation from arm64 to arm64
            "arm64"
        } else {
            panic!("Unsupported host architecture {}", host_architecture);
        }
    } else {
        panic!("Unsupported target architecture.");
    };

    arch_arg.to_owned()
}
