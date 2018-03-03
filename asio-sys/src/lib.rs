mod asio_import;

use std::mem;
use std::os::raw::c_char;
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_long;

use asio_import as ai;

fn setup(){
    /*
#[link(name="libasio")]
    unsafe {
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
    }
    */

}

pub fn get_driver_list() -> Vec<String>{
    let driver_list: Vec<String> = Vec::new();

    let max_names = max_names;
    let mut driver_names: [[c_char; max_names]; max_names] = [[0; max_names]; max_names];
    let mut p_driver_name: [*mut i8; max_names] = [0 as *mut i8; max_names];

    for i in 0..max_names{
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    let mut asio_drivers = sys::AsioDrivers::new();

    let num_drivers = asio_drivers.getDriverNames(p_driver_name.as_mut_ptr(), max_names as i32);

    if num_drivers > 0{
        println!("found driver");
        for i in 0..num_drivers{
            let mut my_driver_name = CString::new("").unwrap();
            let name = CStr::from_ptr(p_driver_name[i as usize]);
            my_driver_name = name.to_owned();
            match my_driver_name.into_string() {
                Ok(s) => driver_list.push(s),
                Err(_) => println!("Failed converting from CString"),
            }
        }
    } else {
        println!("no result");
    }

    sys::destruct_AsioDrivers(&mut asio_drivers);
}


