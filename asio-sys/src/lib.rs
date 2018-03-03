mod asio_import;

use std::os::raw::c_char;
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_long;

use asio_import as ai;

const MAX_DRIVER: i32 = 32;

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
            let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };
            let init_result = ai::ASIOInit(&mut driver_info);
            println!("init result: {}", init_result);
            let channel_result: ai::ASIOError = ai::ASIOGetChannels(&mut ins, &mut outs);
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

    let mut driver_names: [[c_char; MAX_DRIVER]; MAX_DRIVER] = [[0; MAX_DRIVER]; MAX_DRIVER];
    let mut p_driver_name: [*mut i8; MAX_DRIVER] = [0 as *mut i8; MAX_DRIVER];

    for i in 0..MAX_DRIVER{
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    let mut asio_drivers = ai::AsioDrivers::new();

    let num_drivers = asio_drivers.getDriverNames(p_driver_name.as_mut_ptr(), MAX_DRIVER as i32);

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

    ai::destruct_AsioDrivers(&mut asio_drivers);

    driver_list
}


