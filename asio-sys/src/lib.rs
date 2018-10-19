#![allow(non_camel_case_types)]

#[macro_use]
extern crate lazy_static;

extern crate num;
#[macro_use]
extern crate num_derive;

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
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};

use asio_import as ai;

const MAX_DRIVER: usize = 32;

pub struct CbArgs<S, D> {
    pub stream_id: S,
    pub data: D,
}

struct BufferCallback(Box<FnMut(i32) + Send>);

lazy_static!{
    static ref buffer_callback: Mutex<Option<BufferCallback>> = Mutex::new(None);
}

lazy_static!{
    static ref ASIO_DRIVERS: Mutex<Option<DriverWrapper>> = Mutex::new(None);
}

static STREAM_DRIVER_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub struct Channel {
    pub ins: i64,
    pub outs: i64,
}

#[derive(Debug)]
pub struct SampleRate {
    pub rate: u32,
}

#[derive(Debug, Clone)]
pub struct Drivers;

#[derive(Debug)]
struct DriverWrapper {
    pub drivers: ai::AsioDrivers,
}

pub struct AsioStream {
    pub buffer_infos: Vec<AsioBufferInfo>,
    pub buffer_size: i32,
}

#[derive(Debug)]
enum AsioErrorConvert {
	ASE_OK = 0,             // This value will be returned whenever the call succeeded
	ASE_SUCCESS = 0x3f4847a0,	// unique success return value for ASIOFuture calls
	ASE_NotPresent = -1000, // hardware input or output is not present or available
	ASE_HWMalfunction,      // hardware is malfunctioning (can be returned by any ASIO function)
	ASE_InvalidParameter,   // input parameter invalid
	ASE_InvalidMode,        // hardware is in a bad mode or used in a bad mode
	ASE_SPNotAdvancing,     // hardware is not running when sample position is inquired
	ASE_NoClock,            // sample clock or rate cannot be determined or is not present
	ASE_NoMemory,            // not enough memory for completing the request
    Invalid,
}

macro_rules! asio_error {
    ($x:expr, $ae:ident{ $($v:ident),+ }, $inval:ident) => {
        match $x {
            $(_ if $x == $ae::$v as i32 => $ae::$v,)+
            _ => $ae::$inval,
        }
    };
}

fn result_to_error(result: i32) -> AsioErrorConvert {
    asio_error!(result, 
        AsioErrorConvert{
            ASE_OK, 
            ASE_SUCCESS,
            ASE_NotPresent, 
            ASE_HWMalfunction,      
            ASE_InvalidParameter,   
            ASE_InvalidMode,        
            ASE_SPNotAdvancing,     
            ASE_NoClock,            
            ASE_NoMemory}, Invalid)
}

// This is a direct copy of the ASIOSampleType
// inside ASIO SDK.
#[derive(Debug, FromPrimitive)]
#[repr(C)]
pub enum AsioSampleType {
    ASIOSTInt16MSB = 0,
    ASIOSTInt24MSB = 1, // used for 20 bits as well
    ASIOSTInt32MSB = 2,
    ASIOSTFloat32MSB = 3, // IEEE 754 32 bit float
    ASIOSTFloat64MSB = 4, // IEEE 754 64 bit double float

    // these are used for 32 bit data buffer, with different alignment of the data inside
    // 32 bit PCI bus systems can be more easily used with these
    ASIOSTInt32MSB16 = 8,  // 32 bit data with 16 bit alignment
    ASIOSTInt32MSB18 = 9,  // 32 bit data with 18 bit alignment
    ASIOSTInt32MSB20 = 10, // 32 bit data with 20 bit alignment
    ASIOSTInt32MSB24 = 11, // 32 bit data with 24 bit alignment

    ASIOSTInt16LSB = 16,
    ASIOSTInt24LSB = 17, // used for 20 bits as well
    ASIOSTInt32LSB = 18,
    ASIOSTFloat32LSB = 19, // IEEE 754 32 bit float, as found on Intel x86 architecture
    ASIOSTFloat64LSB = 20, // IEEE 754 64 bit double float, as found on Intel x86 architecture

    // these are used for 32 bit data buffer, with different alignment of the data inside
    // 32 bit PCI bus systems can more easily used with these
    ASIOSTInt32LSB16 = 24, // 32 bit data with 18 bit alignment
    ASIOSTInt32LSB18 = 25, // 32 bit data with 18 bit alignment
    ASIOSTInt32LSB20 = 26, // 32 bit data with 20 bit alignment
    ASIOSTInt32LSB24 = 27, // 32 bit data with 24 bit alignment

    //	ASIO DSD format.
    ASIOSTDSDInt8LSB1 = 32, // DSD 1 bit data, 8 samples per byte. First sample in Least significant bit.
    ASIOSTDSDInt8MSB1 = 33, // DSD 1 bit data, 8 samples per byte. First sample in Most significant bit.
    ASIOSTDSDInt8NER8 = 40, // DSD 8 bit data, 1 sample per byte. No Endianness required.

    ASIOSTLastEntry,
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct AsioBufferInfo {
    pub is_input: c_long,
    pub channel_num: c_long,
    pub buffers: [*mut std::os::raw::c_void; 2],
}

#[repr(C)]
struct AsioCallbacks {
    buffer_switch: extern "C" fn(double_buffer_index: c_long, direct_process: c_long) -> (),
    sample_rate_did_change: extern "C" fn(s_rate: c_double) -> (),
    asio_message:
        extern "C" fn(selector: c_long, value: c_long, message: *mut (), opt: *mut c_double)
            -> c_long,
    buffer_switch_time_info: extern "C" fn(
        params: *mut ai::ASIOTime,
        double_buffer_index: c_long,
        direct_process: c_long,
    ) -> *mut ai::ASIOTime,
}

extern "C" fn buffer_switch(double_buffer_index: c_long, direct_process: c_long) -> () {
    let mut bc = buffer_callback.lock().unwrap();

    if let Some(ref mut bc) = *bc {
        bc.run(double_buffer_index);
    }
}

extern "C" fn sample_rate_did_change(s_rate: c_double) -> () {}

extern "C" fn asio_message(
    selector: c_long,
    value: c_long,
    message: *mut (),
    opt: *mut c_double,
) -> c_long {
    4 as c_long
}

extern "C" fn buffer_switch_time_info(
    params: *mut ai::ASIOTime,
    double_buffer_index: c_long,
    direct_process: c_long,
) -> *mut ai::ASIOTime {
    params
}


impl Drivers {
    pub fn load(driver_name: &str) -> Result<Self, ASIOError> {
        let mut drivers = ASIO_DRIVERS.lock().unwrap();
        match *drivers {
            Some(_) => {
                STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(Drivers{})
            },
            None => {
                // Make owned CString to send to load driver
                let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
                let raw = my_driver_name.into_raw();
                let mut driver_info = ai::ASIODriverInfo {
                    _bindgen_opaque_blob: [0u32; 43],
                };
                unsafe {
                    let mut asio_drivers = ai::AsioDrivers::new();
                    let load_result = asio_drivers.loadDriver(raw);
                    ai::ASIOInit(&mut driver_info);
                    // Take back ownership
                    my_driver_name = CString::from_raw(raw);
                    if load_result {
                        println!("Creating drivers");
                        *drivers = Some(DriverWrapper{drivers: asio_drivers});
                        STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                        Ok(Drivers{})
                    } else {
                        Err(ASIOError::DriverLoadError)
                    }
                }
            },
        }
    }

    /// Returns the channels for the driver it's passed
    ///
    /// # Arguments
    /// * `driver name` - Name of the driver
    /// # Usage
    /// Use the get_driver_list() to get the list of names
    /// Then pass the one you want to get_channels
    pub fn get_channels(&self) -> Channel {
        let channel: Channel;

        // Initialize memory for calls
        let mut ins: c_long = 0;
        let mut outs: c_long = 0;
        unsafe {
            println!("Channels result {:?}", result_to_error(ai::ASIOGetChannels(&mut ins, &mut outs)));
            channel = Channel {
                ins: ins as i64,
                outs: outs as i64,
            };
        }

        channel
    }

    pub fn get_sample_rate(&self) -> SampleRate {
        let sample_rate: SampleRate;

        // Initialize memory for calls
        let mut rate: c_double = 0.0f64;

        unsafe {
            println!("sample rate {:?}", result_to_error(ai::get_sample_rate(&mut rate)));
            sample_rate = SampleRate { rate: rate as u32 };
        }

        sample_rate
    }

    pub fn get_data_type(&self) -> Result<AsioSampleType, ASIOError> {
        let data_type: Result<AsioSampleType, ASIOError>;

        // Initialize memory for calls
        let mut channel_info = ai::ASIOChannelInfo {
            channel: 0,
            isInput: 0,
            isActive: 0,
            channelGroup: 0,
            type_: 0,
            name: [0 as c_char; 32],
        };
        unsafe {
            println!("data type {:?}", result_to_error(ai::ASIOGetChannelInfo(&mut channel_info)));
            data_type = num::FromPrimitive::from_i32(channel_info.type_)
                .map_or(Err(ASIOError::TypeError), |t| Ok(t));
        }

        data_type
    }

    pub fn prepare_input_stream(&self, num_channels: usize) -> Result<AsioStream, ASIOError> {
        let mut buffer_infos = vec![
            AsioBufferInfo {
                is_input: 1,
                channel_num: 0,
                buffers: [std::ptr::null_mut(); 2],
            }; num_channels
        ];
        
        let mut callbacks = AsioCallbacks {
            buffer_switch: buffer_switch,
            sample_rate_did_change: sample_rate_did_change,
            asio_message: asio_message,
            buffer_switch_time_info: buffer_switch_time_info,
        };

        let mut min_b_size: c_long = 0;
        let mut max_b_size: c_long = 0;
        let mut pref_b_size: c_long = 0;
        let mut grans: c_long = 0;


        let mut result = Err(ASIOError::NoResult("not implimented".to_owned()));

        unsafe {
            ai::ASIOGetBufferSize(
                &mut min_b_size,
                &mut max_b_size,
                &mut pref_b_size,
                &mut grans,
            );
            result = if pref_b_size > 0 {
                let mut buffer_info_convert: Vec<ai::ASIOBufferInfo> = buffer_infos.into_iter()
                    .map(|bi| mem::transmute::<AsioBufferInfo, ai::ASIOBufferInfo>(bi))
                    .collect();
                let mut callbacks_convert =
                    mem::transmute::<AsioCallbacks, ai::ASIOCallbacks>(callbacks);
                let buffer_result = ai::ASIOCreateBuffers(
                    buffer_info_convert.as_mut_ptr(),
                    num_channels as i32,
                    pref_b_size,
                    &mut callbacks_convert,
                );
                if buffer_result == 0 {
                    let mut buffer_infos: Vec<AsioBufferInfo> = buffer_info_convert.into_iter()
                        .map(|bi| mem::transmute::<ai::ASIOBufferInfo, AsioBufferInfo>(bi))
                        .collect();
                    for d in &buffer_infos {
                        println!("after {:?}", d);
                    }
                    println!("channels: {:?}", num_channels);

                    STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                    return Ok(AsioStream {
                        buffer_infos: buffer_infos,
                        buffer_size: pref_b_size,
                    });
                }
                Err(ASIOError::BufferError(format!(
                    "failed to create buffers, 
                                        error code: {}",
                    buffer_result
                )))
            } else {
                Err(ASIOError::BufferError(
                    "Failed to get buffer size".to_owned(),
                ))
            };
        }
        result
    }

    /// Creates the output stream
    pub fn prepare_output_stream(&self, num_channels: usize) -> Result<AsioStream, ASIOError> {
        // Initialize data for FFI 
        let mut buffer_infos = vec![
            AsioBufferInfo {
                is_input: 0,
                channel_num: 0,
                buffers: [std::ptr::null_mut(); 2],
            }; num_channels
        ];

        let mut callbacks = AsioCallbacks {
            buffer_switch: buffer_switch,
            sample_rate_did_change: sample_rate_did_change,
            asio_message: asio_message,
            buffer_switch_time_info: buffer_switch_time_info,
        };

        let mut min_b_size: c_long = 0;
        let mut max_b_size: c_long = 0;
        let mut pref_b_size: c_long = 0;
        let mut grans: c_long = 0;

        let mut result = Err(ASIOError::NoResult("not implimented".to_owned()));

        unsafe {
            // Get the buffer sizes
            // min possilbe size
            // max possible size
            // preferred size
            // granularity
            ai::ASIOGetBufferSize(
                &mut min_b_size,
                &mut max_b_size,
                &mut pref_b_size,
                &mut grans,
            );
            result = if pref_b_size > 0 {
                /*
                let mut buffer_info_convert = [
                    mem::transmute::<AsioBufferInfo, ai::ASIOBufferInfo>(buffer_infos[0]),
                    mem::transmute::<AsioBufferInfo, ai::ASIOBufferInfo>(buffer_infos[1]),
                ];
                */
                let mut buffer_info_convert: Vec<ai::ASIOBufferInfo> = buffer_infos.into_iter()
                    .map(|bi| mem::transmute::<AsioBufferInfo, ai::ASIOBufferInfo>(bi))
                    .collect();
                let mut callbacks_convert =
                    mem::transmute::<AsioCallbacks, ai::ASIOCallbacks>(callbacks);
                let buffer_result = ai::ASIOCreateBuffers(
                    buffer_info_convert.as_mut_ptr(),
                    num_channels as i32,
                    pref_b_size,
                    &mut callbacks_convert,
                );
                if buffer_result == 0 {
                    /*
                    let buffer_infos = [
                        mem::transmute::<ai::ASIOBufferInfo, AsioBufferInfo>(buffer_info_convert[0]),
                        mem::transmute::<ai::ASIOBufferInfo, AsioBufferInfo>(buffer_info_convert[1]),
                    ];
                    */
                    let mut buffer_infos: Vec<AsioBufferInfo> = buffer_info_convert.into_iter()
                        .map(|bi| mem::transmute::<ai::ASIOBufferInfo, AsioBufferInfo>(bi))
                        .collect();
                    for d in &buffer_infos {
                        println!("after {:?}", d);
                    }
                    println!("channels: {:?}", num_channels);

                    STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                    return Ok(AsioStream {
                        buffer_infos: buffer_infos,
                        buffer_size: pref_b_size,
                    });
                }
                Err(ASIOError::BufferError(format!(
                    "failed to create buffers, error code: {}", buffer_result
                )))
            } else {
                Err(ASIOError::BufferError(
                    "Failed to get buffer size".to_owned(),
                ))
            };
        }
        result
    }
}

impl Drop for Drivers {
    fn drop(&mut self) {
        println!("dropping drivers");
        let count = STREAM_DRIVER_COUNT.fetch_sub(1, Ordering::SeqCst);
        if count == 1 {
            println!("Destroying driver");
            unsafe{
                if let Some(mut asio_drivers) = (*ASIO_DRIVERS.lock().unwrap()).take() {
                    ai::destruct_AsioDrivers(&mut asio_drivers.drivers);
                }
            }
        }
    }
}

impl Drop for AsioStream {
    fn drop(&mut self) {
        println!("dropping stream");
        let count = STREAM_DRIVER_COUNT.fetch_sub(1, Ordering::SeqCst);
        if count == 1 {
            println!("Destroying driver");
            unsafe{
                if let Some(mut asio_drivers) = (*ASIO_DRIVERS.lock().unwrap()).take() {
                    ai::destruct_AsioDrivers(&mut asio_drivers.drivers);
                }
            }
        }
    }
}

unsafe impl Send for DriverWrapper {}

impl BufferCallback {
    fn run(&mut self, index: i32) {
        let mut cb = &mut self.0;
        cb(index);
    }
}

unsafe impl Send for AsioStream {}

pub fn set_callback<F: 'static>(mut callback: F) -> ()
where
    F: FnMut(i32) + Send,
{
    let mut bc = buffer_callback.lock().unwrap();
    *bc = Some(BufferCallback(Box::new(callback)));
}

/// Returns a list of all the ASIO drivers
//TODO this needs to not create and remove drivers
pub fn get_driver_list() -> Vec<String> {
    let mut driver_list: Vec<String> = Vec::new();

    let mut driver_names: [[c_char; MAX_DRIVER]; MAX_DRIVER] = [[0; MAX_DRIVER]; MAX_DRIVER];
    let mut p_driver_name: [*mut i8; MAX_DRIVER] = [0 as *mut i8; MAX_DRIVER];

    for i in 0..MAX_DRIVER {
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    unsafe {
        let mut asio_drivers = ai::AsioDrivers::new();

        let num_drivers =
            asio_drivers.getDriverNames(p_driver_name.as_mut_ptr(), MAX_DRIVER as i32);

        if num_drivers > 0 {
            for i in 0..num_drivers {
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



pub fn destroy_stream(stream: AsioStream) {
    unsafe {
        ai::ASIODisposeBuffers();
        ai::ASIOExit();
    }
}

pub fn play() {
    unsafe {
        let result = ai::ASIOStart();
        println!("start result: {}", result);
    }
}

pub fn stop() {
    unsafe {
        let result = ai::ASIOStop();
        println!("start result: {}", result);
    }
}

