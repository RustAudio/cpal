extern crate asio_sys as sys;
use std::mem;
use std::os::raw::c_char;
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_long;

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
            let mut my_driver_name = CString::new("").unwrap();
            for i in 0..result{
                let name = CStr::from_ptr(p_driver_name[i as usize]);
                println!("Name: {:?}", name);
                my_driver_name = name.to_owned();
            }
            let raw = my_driver_name.into_raw();
            let load_result = asio_drivers.loadDriver(raw);
            my_driver_name = CString::from_raw(raw);
            println!("loaded? {}", load_result);
            if load_result {
                let mut ins: c_long = 0;
                let mut outs: c_long = 0;
                let mut driver_info = sys::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };
                let init_result = sys::ASIOInit(&mut driver_info);
                println!("init result: {}", init_result);
                let channel_result: sys::ASIOError = sys::ASIOGetChannels(&mut ins, &mut outs);
                println!("channel result: {}", channel_result);
                println!("ins: {}", ins);
                println!("outs: {}", outs);
                asio_drivers.removeCurrentDriver();
            }
        } else {
            println!("no result");
        }

        sys::destruct_AsioDrivers(&mut asio_drivers);

    }
    
}
