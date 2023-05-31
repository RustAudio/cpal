/// Simple wrapper around `CreateEventA` as every usage in this crate is the same
#[inline]
pub unsafe fn create_event() -> Result<isize, std::io::Error> {
    let handle =
        super::bindings::CreateEventA(std::ptr::null(), 0, 0, ::windows_core::PCSTR::null());

    if handle != 0 {
        Ok(handle)
    } else {
        Err(std::io::Error::last_os_error())
    }
}
