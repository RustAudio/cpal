mod asio_import;
pub mod errors;

use std::os::raw::c_char;
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::c_long;
use std::os::raw::c_void;
use std::os::raw::c_double;
use errors::ASIOError;
use std::mem;

use asio_import as ai;

const MAX_DRIVER: usize = 32;

#[derive(Debug)]
pub struct Channel{
    pub ins: i64,
    pub outs: i64,
}

#[derive(Debug)]
pub struct SampleRate{
    pub rate: u32,
}

pub struct AsioStream{
    pub buffer_info: AsioBufferInfo,
    driver: ai::AsioDrivers,
}

#[derive(Debug)]
#[repr(C)]
pub struct AsioBufferInfo{
    is_input: c_long,
    channel_num: c_long,
    buffers: [*mut(); 2],
}

#[repr(C)]
struct AsioCallbacks{
    buffer_switch: extern "C" fn(double_buffer_index: c_long,
                                 direct_process: c_long) -> (),
                                 sample_rate_did_change: extern "C" fn(s_rate: c_double) -> (),
                                 asio_message: extern "C" fn(
                                     selector: c_long,
                                     value: c_long,
                                     message: *mut(),
                                     opt: *mut c_double) -> c_long,
                                     buffer_switch_time_info: extern "C" fn(
                                         params: *mut ai::ASIOTime,
                                         double_buffer_index: c_long,
                                         direct_process: c_long) -> *mut ai::ASIOTime,
}

extern "C" fn buffer_switch(double_buffer_index: c_long,
                       direct_process: c_long) -> (){
    println!("index: {}", double_buffer_index);
    println!("direct_process: {}", direct_process);
}

extern "C" fn sample_rate_did_change(s_rate: c_double) -> (){
}

extern "C" fn asio_message(selector: c_long,
        value: c_long,
        message: *mut(),
        opt: *mut c_double) -> c_long{
    4 as c_long
}

extern "C" fn buffer_switch_time_info(params: *mut ai::ASIOTime,
                               double_buffer_index: c_long,
                               direct_process: c_long) -> *mut ai::ASIOTime{
    params
}

impl AsioStream {
    fn pop_driver(self) -> ai::AsioDrivers{
        self.driver
    }
}

/// Returns the channels for the driver it's passed
///
/// # Arguments
/// * `driver name` - Name of the driver
/// # Usage
/// Use the get_driver_list() to get the list of names
/// Then pass the one you want to get_channels
pub fn get_channels(driver_name: &str) -> Result<Channel, ASIOError>{
    let channel: Result<Channel, ASIOError>;
    // Make owned CString to send to load driver
    let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
    let raw = my_driver_name.into_raw();

    // Initialize memory for calls
    let mut ins: c_long = 0;
    let mut outs: c_long = 0;
    let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };

    unsafe{
        let mut asio_drivers = ai::AsioDrivers::new();

        let load_result = asio_drivers.loadDriver(raw);

        // Take back ownership
        my_driver_name = CString::from_raw(raw);

        if load_result {
            ai::ASIOInit(&mut driver_info);
            ai::ASIOGetChannels(&mut ins, &mut outs);
            asio_drivers.removeCurrentDriver();
            channel = Ok(Channel{ ins: ins as i64, outs: outs as i64});
        }else{
            channel = Err(ASIOError::NoResult(driver_name.to_owned()));
        }
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }
    
    channel
}

/// Returns a list of all the ASIO drivers
pub fn get_driver_list() -> Vec<String>{
    let mut driver_list: Vec<String> = Vec::new();

    let mut driver_names: [[c_char; MAX_DRIVER]; MAX_DRIVER] = [[0; MAX_DRIVER]; MAX_DRIVER];
    let mut p_driver_name: [*mut i8; MAX_DRIVER] = [0 as *mut i8; MAX_DRIVER];

    for i in 0..MAX_DRIVER{
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    unsafe{

        let mut asio_drivers = ai::AsioDrivers::new();

        let num_drivers = asio_drivers.getDriverNames(p_driver_name.as_mut_ptr(), MAX_DRIVER as i32);

        if num_drivers > 0{
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
            println!("No ASIO drivers found");
        }

        ai::destruct_AsioDrivers(&mut asio_drivers);
    }

    driver_list
}


pub fn get_sample_rate(driver_name: &str) -> Result<SampleRate, ASIOError>{
    
    let sample_rate: Result<SampleRate, ASIOError>;
    // Make owned CString to send to load driver
    let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
    let raw = my_driver_name.into_raw();

    // Initialize memory for calls
    let mut rate: c_double = 0.0f64; 
    let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };

    unsafe{
        let mut asio_drivers = ai::AsioDrivers::new();

        let load_result = asio_drivers.loadDriver(raw);

        // Take back ownership
        my_driver_name = CString::from_raw(raw);

        if load_result {
            ai::ASIOInit(&mut driver_info);
            ai::get_sample_rate(&mut rate);
            asio_drivers.removeCurrentDriver();
            sample_rate = Ok(SampleRate{ rate: rate as u32});
        }else{
            sample_rate = Err(ASIOError::NoResult(driver_name.to_owned()));
        }
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }
    
    sample_rate
}

pub fn prepare_stream(driver_name: &str) -> Result<AsioStream, ASIOError>{
    //let mut buffer_info = ai::ASIOBufferInfo{_bindgen_opaque_blob: [0u32; 6]};
    let mut buffer_info = AsioBufferInfo{ 
        is_input: 0, 
        channel_num: 0,
        buffers: [std::ptr::null_mut(); 2]
    };

    let num_channels = 2;
    //let mut callbacks = ai::ASIOCallbacks{_bindgen_opaque_blob: [0u32; 8]};
    let mut callbacks = AsioCallbacks{
        buffer_switch: buffer_switch,
        sample_rate_did_change: sample_rate_did_change,
        asio_message: asio_message,
        buffer_switch_time_info: buffer_switch_time_info
    };

    let mut min_b_size: c_long = 0;
    let mut max_b_size: c_long = 0;
    let mut pref_b_size: c_long = 0;
    let mut grans: c_long = 0;

    let mut driver_info = ai::ASIODriverInfo{_bindgen_opaque_blob: [0u32; 43] };
    
    // Make owned CString to send to load driver
    let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
    let raw = my_driver_name.into_raw();

    let mut result = Err(ASIOError::NoResult("not implimented".to_owned()));

    unsafe{
        let mut asio_drivers = ai::AsioDrivers::new();
        let load_result = asio_drivers.loadDriver(raw);
        // Take back ownership
        my_driver_name = CString::from_raw(raw);
        if !load_result { return Err(ASIOError::DriverLoadError); }


        ai::ASIOInit(&mut driver_info);
        ai::ASIOGetBufferSize(&mut min_b_size, &mut max_b_size,
                              &mut pref_b_size, &mut grans);
        result = if  pref_b_size > 0 { 
            let mut buffer_info_convert = mem::transmute::<AsioBufferInfo, 
            ai::ASIOBufferInfo>(buffer_info);
            let mut callbacks_convert = mem::transmute::<AsioCallbacks,
            ai::ASIOCallbacks>(callbacks);
            let buffer_result = ai::ASIOCreateBuffers(&mut buffer_info_convert, 
                                                             num_channels,
                                                             pref_b_size, 
                                                             &mut callbacks_convert);
            if buffer_result == 0{
                return Ok(AsioStream{ 
                    buffer_info: mem::transmute::<ai::ASIOBufferInfo,
                    AsioBufferInfo>(buffer_info_convert),
                    driver: asio_drivers
                })
            }
            Err(ASIOError::BufferError(format!("failed to create buffers, 
                                       error code: {}", buffer_result)))
        }else{
            Err(ASIOError::BufferError("Failed to get buffer size".to_owned()))
        };

        asio_drivers.removeCurrentDriver();
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }
    result
}

pub fn destroy_stream(stream: AsioStream) {
    unsafe{
        ai::ASIODisposeBuffers();
        let mut asio_drivers = stream.pop_driver();
        asio_drivers.removeCurrentDriver();
        ai::destruct_AsioDrivers(&mut asio_drivers);
    }
}

pub fn play(){
    unsafe{
        let result = ai::ASIOStart();
        println!("start result: {}", result);
    }
}

pub fn stop(){
    unsafe{
        let result = ai::ASIOStop();
        println!("start result: {}", result);
    }
}
