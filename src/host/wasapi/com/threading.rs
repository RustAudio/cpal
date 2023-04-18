#[inline]
#[allow(non_snake_case)]
pub unsafe fn CreateEventA(
    attrs: Option<*const std::ffi::c_void>,
    manual_reset: bool,
    initial_state: bool,
    name: Option<*const u8>,
) -> Result<isize, std::io::Error> {
    // We manually create this extern so that we don't have to include the "Security" feature,
    // which adds 70 structs, 394 constants, 133 functions, 31 type aliases, and
    // 5 unions....none of which this crate uses
    windows_targets::link!("kernel32.dll" "system" fn CreateEventA(lpeventattributes: *const std::ffi::c_void, manualreset: i32, initialstate: i32, name: windows_sys::core::PCSTR) -> isize);

    let handle = CreateEventA(
        attrs.unwrap_or(std::ptr::null()),
        manual_reset as _,
        initial_state as _,
        name.unwrap_or(std::ptr::null()),
    );

    if handle != 0 {
        Ok(handle)
    } else {
        Err(std::io::Error::last_os_error())
    }
}
