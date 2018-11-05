#![allow(non_camel_case_types)]

#[macro_use]
extern crate lazy_static;

extern crate num;
#[macro_use]
extern crate num_derive;

mod asio_import;
#[macro_use]
pub mod errors;

use errors::{AsioDriverError, AsioError, AsioErrorWrapper};
use std::ffi::CStr;
use std::ffi::CString;
use std::mem;
use std::os::raw::{c_char, c_double, c_long, c_void};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

// Bindings import
use asio_import as ai;

// TODO I dont think this is needed anymore
pub struct CbArgs<S, D> {
    pub stream_id: S,
    pub data: D,
}

/// Holds the pointer to the callbacks that come from cpal
struct BufferCallback(Box<FnMut(i32) + Send>);

/// A global way to access all the callbacks.
/// This is required because of how ASIO
/// calls the buffer_switch function.
/// Options are used so that when a callback is
/// removed we don't change the Vec indicies.
/// The indicies are how we match a callback
/// with a stream.
lazy_static! {
    static ref buffer_callback: Mutex<Vec<Option<BufferCallback>>> = Mutex::new(Vec::new());
}

/// Globally available state of the ASIO driver.
/// This allows all calls the the driver to ensure 
/// they are calling in the correct state.
/// It also prevents multiple calls happening at once.
lazy_static! {
    static ref ASIO_DRIVERS: Mutex<AsioWrapper> = Mutex::new(AsioWrapper {
        state: AsioState::Offline,
    });
}

/// Count of active device and streams.
/// Used to clean up the driver connection 
/// when there are no active connections.
static STREAM_DRIVER_COUNT: AtomicUsize = AtomicUsize::new(0);
/// Tracks which buffer needs to be silenced.
pub static SILENCE_FIRST: AtomicBool = AtomicBool::new(false);
pub static SILENCE_SECOND: AtomicBool = AtomicBool::new(false);

/// Amount of input and output 
/// channels available.
#[derive(Debug)]
pub struct Channel {
    pub ins: i64,
    pub outs: i64,
}

/// Sample rate of the ASIO device.
#[derive(Debug)]
pub struct SampleRate {
    pub rate: u32,
}

/// A marker type to make sure
/// all calls to the driver have an
/// active connection.
#[derive(Debug, Clone)]
pub struct Drivers;

/// Tracks the current state of the 
/// ASIO drivers.
#[derive(Debug)]
struct AsioWrapper {
    state: AsioState,
}

/// All possible states of the 
/// ASIO driver. Mapped to the 
/// FSM in the ASIO SDK docs.
#[derive(Debug)]
enum AsioState {
    Offline,
    Loaded,
    Initialized,
    Prepared,
    Running,
}

/// Input and Output streams.
/// There is only ever max one
/// input and one output. Only one
/// is required.
pub struct AsioStreams {
    pub input: Option<AsioStream>,
    pub output: Option<AsioStream>,
}

/// A stream to ASIO.
/// Contains the buffers.
pub struct AsioStream {
    /// A Double buffer per channel
    pub buffer_infos: Vec<AsioBufferInfo>,
    /// Size of each buffer
    pub buffer_size: i32,
}

/// All the possible types from ASIO.
/// This is a direct copy of the ASIOSampleType
/// inside ASIO SDK. 
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

/// Gives information about buffers
/// Receives pointers to buffers
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct AsioBufferInfo {
    /// 0 for output 1 for input
    pub is_input: c_long,
    /// Which channel. Starts at 0
    pub channel_num: c_long,
    /// Pointer to each half of the double buffer.
    pub buffers: [*mut c_void; 2],
}

/// Callbacks that ASIO calls
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

/// This is called by ASIO.
/// Here we run the callback for each stream.
/// double_buffer_index is either 0 or 1
/// indicating which buffer to fill
extern "C" fn buffer_switch(double_buffer_index: c_long, _direct_process: c_long) -> () {
    // This lock is probably unavoidable 
    // but locks in the audio stream is not great
    let mut bcs = buffer_callback.lock().unwrap();

    for mut bc in bcs.iter_mut() {
        if let Some(ref mut bc) = bc {
            bc.run(double_buffer_index);
        }
    }
}

/// Idicates the sample rate has changed
/// TODO Change the sample rate when this
/// is called.
extern "C" fn sample_rate_did_change(_s_rate: c_double) -> () {}

/// Messages for ASIO
/// This is not currently used
extern "C" fn asio_message(
    _selector: c_long, _value: c_long, _message: *mut (), _opt: *mut c_double,
) -> c_long {
    // TODO Impliment this to give proper responses
    4 as c_long
}

/// Similar to buffer switch but with time info
/// Not currently used
extern "C" fn buffer_switch_time_info(
    params: *mut ai::ASIOTime, _double_buffer_index: c_long, _direct_process: c_long,
) -> *mut ai::ASIOTime {
    params
}

/// Helper function for getting the drivers.
/// Note this is a lock.
fn get_drivers() -> MutexGuard<'static, AsioWrapper> {
    ASIO_DRIVERS.lock().unwrap()
}

impl Drivers {
    /// Load the drivers from a driver name.
    /// This will destroy the old drivers.
    #[allow(unused_assignments)]
    pub fn load(driver_name: &str) -> Result<Self, AsioDriverError> {
        let mut drivers = get_drivers();
        // Make owned CString to send to load driver
        let mut my_driver_name = CString::new(driver_name).expect("Can't go from str to CString");
        let raw = my_driver_name.into_raw();
        let mut driver_info = ai::ASIODriverInfo {
            _bindgen_opaque_blob: [0u32; 43],
        };
        unsafe {
            // Destroy old drivers and load new drivers.
            let load_result = drivers.load(raw);
            // Take back ownership
            my_driver_name = CString::from_raw(raw);
            if load_result {
                // Initialize ASIO
                match drivers.asio_init(&mut driver_info) {
                    Ok(_) => {
                        // If it worked then add a active connection to the drivers
                        // TODO Make sure this is decremented when old drivers are dropped
                        STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                        Ok(Drivers)
                    },
                    Err(_) => Err(AsioDriverError::DriverLoadError),
                }
            } else {
                Err(AsioDriverError::DriverLoadError)
            }
        }
    }

    /// Returns the number of input and output 
    /// channels for the active drivers
    pub fn get_channels(&self) -> Channel {
        let channel: Channel;

        // Initialize memory for calls
        let mut ins: c_long = 0;
        let mut outs: c_long = 0;
        unsafe {
            get_drivers()
                .asio_get_channels(&mut ins, &mut outs)
                // TODO pass this result along
                // and handle it without panic
                .expect("failed to get channels");
            channel = Channel {
                ins: ins as i64,
                outs: outs as i64,
            };
        }

        channel
    }

    /// Get current sample rate of the active drivers
    pub fn get_sample_rate(&self) -> SampleRate {
        let sample_rate: SampleRate;

        // Initialize memory for calls
        let mut rate: c_double = 0.0f64;

        unsafe {
            get_drivers()
                .asio_get_sample_rate(&mut rate)
                // TODO pass this result along
                // and handle it without panic
                .expect("failed to get sample rate");
            sample_rate = SampleRate { rate: rate as u32 };
        }

        sample_rate
    }

    /// Set the sample rate for the active drivers
    pub fn set_sample_rate(&self, sample_rate: u32) -> Result<(), AsioError> {
        // Initialize memory for calls
        let rate: c_double = c_double::from(sample_rate);

        unsafe { get_drivers().asio_set_sample_rate(rate) }
    }

    /// Can the drivers accept the given sample rate
    pub fn can_sample_rate(&self, sample_rate: u32) -> bool {
        // Initialize memory for calls
        let rate: c_double = c_double::from(sample_rate);

        // TODO this gives an error is it can't handle the sample
        // rate but it can also give error for no divers
        // Both should be handled.
        unsafe { get_drivers().asio_can_sample_rate(rate).is_ok() }
    }

    /// Get the current data type of the active drivers.
    /// Just queries a single channels type as all channels
    /// have the same sample type.
    pub fn get_data_type(&self) -> Result<AsioSampleType, AsioDriverError> {
        // TODO make this a seperate call for input and output as
        // it is possible that input and output have different sample types
        // Initialize memory for calls
        let mut channel_info = ai::ASIOChannelInfo {
            // Which channel we are querying
            channel: 0,
            // Was it input or output
            isInput: 0,
            // Was it active
            isActive: 0,
            channelGroup: 0,
            // The sample type
            type_: 0,
            name: [0 as c_char; 32],
        };
        unsafe {
            match get_drivers().asio_get_channel_info(&mut channel_info) {
                Ok(_) => num::FromPrimitive::from_i32(channel_info.type_)
                    .map_or(Err(AsioDriverError::TypeError), |t| Ok(t)),
                Err(e) => {
                    println!("Error getting data type {}", e);
                    Err(AsioDriverError::DriverLoadError)
                },
            }
        }
    }

    /// Prepare the input stream.
    /// Because only the latest call
    /// to ASIOCreateBuffers is relevant this
    /// call will destroy all past active buffers
    /// and recreate them. For this reason we take 
    /// the output stream if it exists.
    /// num_channels is the number of input channels.
    /// This returns a full AsioStreams with both input
    /// and output if output was active.
    pub fn prepare_input_stream(
        &self, output: Option<AsioStream>, num_channels: usize,
    ) -> Result<AsioStreams, AsioDriverError> {
        let buffer_infos = (0 .. num_channels)
            .map(|i| AsioBufferInfo {
                // These are output channels
                is_input: 1,
                // Channel index
                channel_num: i as c_long,
                // Double buffer. We don't know the type
                // at this point
                buffers: [std::ptr::null_mut(); 2],
            }).collect();

        // Create the streams
        let streams = AsioStreams {
            input: Some(AsioStream {
                buffer_infos,
                buffer_size: 0,
            }),
            output,
        };
        self.create_streams(streams)
    }

    /// Prepare the output stream.
    /// Because only the latest call
    /// to ASIOCreateBuffers is relevant this
    /// call will destroy all past active buffers
    /// and recreate them. For this reason we take 
    /// the input stream if it exists.
    /// num_channels is the number of output channels.
    /// This returns a full AsioStreams with both input
    /// and output if input was active.
    pub fn prepare_output_stream(
        &self, input: Option<AsioStream>, num_channels: usize,
    ) -> Result<AsioStreams, AsioDriverError> {
        // Initialize data for FFI
        let buffer_infos = (0 .. num_channels)
            .map(|i| AsioBufferInfo {
                // These are outputs
                is_input: 0,
                // Channel index
                channel_num: i as c_long,
                // Pointer to each buffer. We don't know
                // the type yet.
                buffers: [std::ptr::null_mut(); 2],
            }).collect();

        // Create streams
        let streams = AsioStreams {
            output: Some(AsioStream {
                buffer_infos,
                buffer_size: 0,
            }),
            input,
        };
        self.create_streams(streams)
    }

    /// Creates the streams.
    /// Both input and output streams
    /// need to be created together as
    /// a single slice of ASIOBufferInfo
    fn create_streams(&self, streams: AsioStreams) -> Result<AsioStreams, AsioDriverError> {
        let AsioStreams { input, output } = streams;
        match (input, output) {
            // Both stream exist.
            (Some(input), Some(mut output)) => {
                let split_point = input.buffer_infos.len();
                let mut bi = input.buffer_infos;
                // Append the output to the input channels
                bi.append(&mut output.buffer_infos);
                // Create the buffers.
                // if successful then split the output
                // and input again
                self.create_buffers(bi).map(|(mut bi, buffer_size)| {
                    let out_bi = bi.split_off(split_point);
                    let in_bi = bi;
                    let output = Some(AsioStream {
                        buffer_infos: out_bi,
                        buffer_size,
                    });
                    let input = Some(AsioStream {
                        buffer_infos: in_bi,
                        buffer_size,
                    });
                    AsioStreams { output, input }
                })
            },
            // Just input
            (Some(input), None) => {
                self.create_buffers(input.buffer_infos)
                    .map(|(buffer_infos, buffer_size)| {
                        STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                        AsioStreams {
                            input: Some(AsioStream {
                                buffer_infos,
                                buffer_size,
                            }),
                            output: None,
                        }
                    })
            },
            // Just output
            (None, Some(output)) => {
                self.create_buffers(output.buffer_infos)
                    .map(|(buffer_infos, buffer_size)| {
                        STREAM_DRIVER_COUNT.fetch_add(1, Ordering::SeqCst);
                        AsioStreams {
                            output: Some(AsioStream {
                                buffer_infos,
                                buffer_size,
                            }),
                            input: None,
                        }
                    })
            },
            // Impossible
            (None, None) => panic!("Trying to create streams without preparing"),
        }
    }

    /// Ask ASIO to allocate the buffers
    /// and give the callback pointers.
    /// This will destroy any already allocated 
    /// buffers.
    /// The prefered buffer size from ASIO is used.
    fn create_buffers(
        &self, buffer_infos: Vec<AsioBufferInfo>,
    ) -> Result<(Vec<AsioBufferInfo>, c_long), AsioDriverError> {
        let num_channels = buffer_infos.len();
        let callbacks = AsioCallbacks {
            buffer_switch: buffer_switch,
            sample_rate_did_change: sample_rate_did_change,
            asio_message: asio_message,
            buffer_switch_time_info: buffer_switch_time_info,
        };

        let mut min_b_size: c_long = 0;
        let mut max_b_size: c_long = 0;
        let mut pref_b_size: c_long = 0;
        let mut grans: c_long = 0;

        let mut drivers = get_drivers();

        unsafe {
            // Get the buffer sizes
            // min possilbe size
            // max possible size
            // preferred size
            // granularity
            drivers
                .asio_get_buffer_size(
                    &mut min_b_size,
                    &mut max_b_size,
                    &mut pref_b_size,
                    &mut grans,
                ).expect("Failed getting buffers");
            if pref_b_size > 0 {
                // Convert Rust structs to opaque ASIO structs
                let mut buffer_info_convert =
                    mem::transmute::<Vec<AsioBufferInfo>, Vec<ai::ASIOBufferInfo>>(buffer_infos);
                let mut callbacks_convert =
                    mem::transmute::<AsioCallbacks, ai::ASIOCallbacks>(callbacks);
                drivers
                    .asio_create_buffers(
                        buffer_info_convert.as_mut_ptr(),
                        num_channels as i32,
                        pref_b_size,
                        &mut callbacks_convert,
                    ).map(|_| {
                        let buffer_infos = mem::transmute::<
                            Vec<ai::ASIOBufferInfo>,
                            Vec<AsioBufferInfo>,
                        >(buffer_info_convert);
                        (buffer_infos, pref_b_size)
                    }).map_err(|e| {
                        AsioDriverError::BufferError(format!(
                            "failed to create buffers, error code: {:?}",
                            e
                        ))
                    })
            } else {
                Err(AsioDriverError::BufferError("bad buffer size".to_owned()))
            }
        }
    }
}

/// If all drivers and streams are gone
/// clean up drivers
impl Drop for Drivers {
    fn drop(&mut self) {
        let count = STREAM_DRIVER_COUNT.fetch_sub(1, Ordering::SeqCst);
        if count == 1 {
            clean_up();
        }
    }
}

/// Required for Mutex
unsafe impl Send for AsioWrapper {}
/// Required for Mutex
unsafe impl Send for AsioStream {}

impl BufferCallback {
    /// Calls the inner callback
    fn run(&mut self, index: i32) {
        let cb = &mut self.0;
        cb(index);
    }
}


/// Adds a callback to the list of active callbacks
pub fn set_callback<F: 'static>(callback: F) -> ()
where
    F: FnMut(i32) + Send,
{
    let mut bc = buffer_callback.lock().unwrap();
    bc.push(Some(BufferCallback(Box::new(callback))));
}

/// Returns a list of all the ASIO devices.
/// This is used at the start to allow the
/// user to choose which device they want.
#[allow(unused_assignments)]
pub fn get_driver_list() -> Vec<String> {
    // The most devices we can take
    const MAX_DRIVERS: usize = 100;
    // Max length for divers name
    const CHAR_LEN: usize = 32;

    // 2D array of driver names set to 0
    let mut driver_names: [[c_char; CHAR_LEN]; MAX_DRIVERS] = [[0; CHAR_LEN]; MAX_DRIVERS];
    // Pointer to each driver name
    let mut p_driver_name: [*mut i8; MAX_DRIVERS] = [0 as *mut i8; MAX_DRIVERS];

    for i in 0 .. MAX_DRIVERS {
        p_driver_name[i] = driver_names[i].as_mut_ptr();
    }

    unsafe {
        let num_drivers = ai::get_driver_names(p_driver_name.as_mut_ptr(), MAX_DRIVERS as i32);

        (0 .. num_drivers)
            .map(|i| {
                let mut my_driver_name = CString::new("").unwrap();
                let name = CStr::from_ptr(p_driver_name[i as usize]);
                my_driver_name = name.to_owned();
                my_driver_name
                    .into_string()
                    .expect("Failed to convert driver name")
            }).collect()
    }
}

/// Cleans up the drivers and
/// any allocations
pub fn clean_up() {
    let mut drivers = get_drivers();
    drivers.clean_up();
}

/// Starts input and output streams playing
pub fn play() {
    unsafe {
        // TODO handle result instead of printing
        let result = get_drivers().asio_start();
        println!("start result: {:?}", result);
    }
}

/// Stops input and output streams playing
pub fn stop() {
    unsafe {
        // TODO handle result instead of printing
        let result = get_drivers().asio_stop();
        println!("stop result: {:?}", result);
    }
}

/// All the actual calls to ASIO.
/// This is where we handle the state
/// and assert that all calls
/// happen in the correct state.
/// TODO it is possible to enforce most of this
/// at compile time using type parameters.
/// All calls have results that are converted
/// to Rust style results.
impl AsioWrapper {
    /// Load the driver.
    /// Unloads the previous driver.
    /// Sets state to Loaded on success.
    unsafe fn load(&mut self, raw: *mut i8) -> bool {
        use AsioState::*;
        self.clean_up();
        if ai::load_asio_driver(raw) {
            self.state = Loaded;
            true
        } else {
            false
        }
    }

    /// Unloads the current driver from ASIO
    unsafe fn unload(&mut self) {
        ai::remove_current_driver();
    }

    /// Initializes ASIO.
    /// Needs to be already Loaded.
    /// Initialized on success.
    /// No-op if already Initialized or higher.
    /// TODO should fail if Offline
    unsafe fn asio_init(&mut self, di: &mut ai::ASIODriverInfo) -> Result<(), AsioError> {
        if let AsioState::Loaded = self.state {
            let result = ai::ASIOInit(di);
            asio_result!(result)
                .map(|_| self.state = AsioState::Initialized)
        } else {
            Ok(())
        }
    }

    /// Gets the number of channels.
    /// Needs to be atleast Loaded.
    unsafe fn asio_get_channels(
        &mut self, ins: &mut c_long, outs: &mut c_long,
    ) -> Result<(), AsioError> {
        if let AsioState::Offline = self.state {
            Err(AsioError::NoDrivers)
        } else {
            let result = ai::ASIOGetChannels(ins, outs);
            asio_result!(result)
        }
    }

    /// Gets the sample rate.
    /// Needs to be atleast Loaded.
    unsafe fn asio_get_sample_rate(&mut self, rate: &mut c_double) -> Result<(), AsioError> {
        if let AsioState::Offline = self.state {
            Err(AsioError::NoDrivers)
        } else {
            let result = ai::get_sample_rate(rate);
            asio_result!(result)
        }
    }

    /// Sets the sample rate.
    /// Needs to be atleast Loaded.
    unsafe fn asio_set_sample_rate(&mut self, rate: c_double) -> Result<(), AsioError> {
        if let AsioState::Offline = self.state {
            Err(AsioError::NoDrivers)
        } else {
            let result = ai::set_sample_rate(rate);
            asio_result!(result)
        }
    }

    /// Queries if the sample rate is possible.
    /// Needs to be atleast Loaded.
    unsafe fn asio_can_sample_rate(&mut self, rate: c_double) -> Result<(), AsioError> {
        if let AsioState::Offline = self.state {
            Err(AsioError::NoDrivers)
        } else {
            let result = ai::can_sample_rate(rate);
            asio_result!(result)
        }
    }

    /// Get information about a channel.
    /// Needs to be atleast Loaded.
    unsafe fn asio_get_channel_info(
        &mut self, ci: &mut ai::ASIOChannelInfo,
    ) -> Result<(), AsioError> {
        if let AsioState::Offline = self.state {
            Err(AsioError::NoDrivers)
        } else {
            let result = ai::ASIOGetChannelInfo(ci);
            asio_result!(result)
        }
    }
    
    /// Gets the buffer sizes.
    /// Needs to be atleast Loaded.
    unsafe fn asio_get_buffer_size(
        &mut self, min_b_size: &mut c_long, max_b_size: &mut c_long, pref_b_size: &mut c_long,
        grans: &mut c_long,
    ) -> Result<(), AsioError> {
        if let AsioState::Offline = self.state {
            Err(AsioError::NoDrivers)
        } else {
            let result = ai::ASIOGetBufferSize(min_b_size, max_b_size, pref_b_size, grans);
            asio_result!(result)
        }
    }

    /// Creates the buffers.
    /// Needs to be atleast Loaded.
    /// If Running or Prepared then old buffers
    /// will be destoryed.
    unsafe fn asio_create_buffers(
        &mut self, buffer_info_convert: *mut ai::ASIOBufferInfo, num_channels: i32,
        pref_b_size: c_long, callbacks_convert: &mut ai::ASIOCallbacks,
    ) -> Result<(), AsioError> {
        use AsioState::*;
        match self.state {
            Offline | Loaded => return Err(AsioError::NoDrivers),
            Running => {
                self.asio_stop().expect("Asio failed to stop");
                self.asio_dispose_buffers()
                    .expect("Failed to dispose buffers");
                self.state = Initialized;
            },
            Prepared => {
                self.asio_dispose_buffers()
                    .expect("Failed to dispose buffers");
                self.state = Initialized;
            },
            _ => (),
        }
        let result = ai::ASIOCreateBuffers(
            buffer_info_convert,
            num_channels,
            pref_b_size,
            callbacks_convert,
        );
        asio_result!(result).map(|_| self.state = AsioState::Prepared)
    }

    /// Releases buffers allocations.
    /// Needs to be atleast Loaded.
    /// No op if already released.
    unsafe fn asio_dispose_buffers(&mut self) -> Result<(), AsioError> {
        use AsioState::*;
        match self.state {
            Offline | Loaded => return Err(AsioError::NoDrivers),
            Running | Prepared => (),
            Initialized => return Ok(()),
        }
        let result = ai::ASIODisposeBuffers();
        asio_result!(result).map(|_| self.state = AsioState::Initialized)
    }

    /// Closes down ASIO.
    /// Needs to be atleast Loaded.
    unsafe fn asio_exit(&mut self) -> Result<(), AsioError> {
        use AsioState::*;
        match self.state {
            Offline | Loaded => return Err(AsioError::NoDrivers),
            _ => (),
        }
        let result = ai::ASIOExit();
        asio_result!(result).map(|_| self.state = AsioState::Offline)
    }

    /// Starts ASIO streams playing.
    /// Needs to be atleast Initialized.
    unsafe fn asio_start(&mut self) -> Result<(), AsioError> {
        use AsioState::*;
        match self.state {
            Offline | Loaded | Initialized => return Err(AsioError::NoDrivers),
            Running => return Ok(()),
            Prepared => (),
        }
        let result = ai::ASIOStart();
        asio_result!(result).map(|_| self.state = AsioState::Running)
    }

    /// Stops ASIO streams playing.
    /// Needs to be Running.
    /// No-op if already stopped.
    unsafe fn asio_stop(&mut self) -> Result<(), AsioError> {
        use AsioState::*;
        match self.state {
            Offline | Loaded => return Err(AsioError::NoDrivers),
            Running => (),
            Initialized | Prepared => return Ok(()),
        }
        let result = ai::ASIOStop();
        asio_result!(result).map(|_| self.state = AsioState::Prepared)
    }

    /// Cleans up the drivers based
    /// on the current state of the driver.
    fn clean_up(&mut self) {
        match self.state {
            AsioState::Offline => (),
            AsioState::Loaded => {
                unsafe {
                    self.asio_exit().expect("Failed to exit asio");
                    self.unload();
                }
                self.state = AsioState::Offline;
            },
            AsioState::Initialized => {
                unsafe {
                    self.asio_exit().expect("Failed to exit asio");
                    self.unload();
                }
                self.state = AsioState::Offline;
            },
            AsioState::Prepared => {
                unsafe {
                    self.asio_dispose_buffers()
                        .expect("Failed to dispose buffers");
                    self.asio_exit().expect("Failed to exit asio");
                    self.unload();
                }
                self.state = AsioState::Offline;
            },
            AsioState::Running => {
                unsafe {
                    self.asio_stop().expect("Asio failed to stop");
                    self.asio_dispose_buffers()
                        .expect("Failed to dispose buffers");
                    self.asio_exit().expect("Failed to exit asio");
                    self.unload();
                }
                self.state = AsioState::Offline;
            },
        }
    }
}
