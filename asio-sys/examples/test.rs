extern crate asio_sys as sys;
use std::mem;

fn main() {
    unsafe {
        let mut asio_drivers = mem::uninitialized();

        sys::AsioDrivers_AsioDrivers(&mut asio_drivers);
        let mut current_driver_name: [std::os::raw::c_char; 32] = [0; 32];
        let result = sys::AsioDrivers_getCurrentDriverName(
            &mut asio_drivers,
            current_driver_name.as_mut_ptr(),
        );
        if result {
            println!("current driver: {:?}", current_driver_name);
        } else {
            println!("no result");
        }
        sys::AsioDrivers_AsioDrivers_destructor(&mut asio_drivers);
    }
}
