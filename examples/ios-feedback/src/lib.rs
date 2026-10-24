mod feedback;

// Required for Xcode to link this entry point from Objective-C.
#[unsafe(no_mangle)]
pub extern "C" fn rust_ios_main() {
    feedback::run_example().unwrap();
}
