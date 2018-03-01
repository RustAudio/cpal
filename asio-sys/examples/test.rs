extern crate asio_sys as sys;
use std::os::raw::c_char;
use std::ffi::CStr;

fn main() {
    
    let max_names = 32;
    let mut driver_names: [[c_char; 32]; 32] = [[0; 32]; 32];
    let mut p_driver_name: [*mut i8; 32] = [0 as *mut i8; 32];

    for i in 0..max_names{
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    #[link(name="libasio")]
    unsafe {
        let mut asio_drivers = sys::AsioDrivers::new();

        let result = asio_drivers.getDriverNames(p_driver_name.as_mut_ptr(), max_names as i32);

        if result > 0{
            println!("found driver");
            for i in 0..result{
                let name = CStr::from_ptr(p_driver_name[i as usize]);
                println!("Name: {:?}", name);
            }
        } else {
            println!("no result");
        }

        sys::destruct_AsioDrivers(&mut asio_drivers);

    }
    
}
