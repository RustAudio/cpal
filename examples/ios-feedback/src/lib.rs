mod feedback;

#[no_mangle]
pub extern "C" fn rust_ios_main() {
    feedback::run_example().unwrap();
}
