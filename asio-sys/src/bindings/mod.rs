mod asio_import;
#[macro_use]
pub mod errors;

use num_traits::FromPrimitive;
use self::errors::{AsioError, AsioErrorWrapper, LoadDriverError};
use std::ffi::CStr;
use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_long, c_void};
use std::sync::{Arc, Mutex, Weak};

// Bindings import
use self::asio_import as ai;

/// A handle to the ASIO API.
///
/// There should only be one instance of this type at any point in time.
#[derive(Debug)]
pub struct Asio {
    // Keeps track of whether or not a driver is already loaded.
    //
    // This is necessary as ASIO only supports one `Driver` at a time.
    loaded_driver: Mutex<Weak<DriverInner>>,
}

/// A handle to a single ASIO driver.
///
/// Creating an instance of this type loads and initialises the driver.
///
/// Dropping all `Driver` instances will automatically dispose of any resources and de-initialise
/// the driver.
#[derive(Clone, Debug)]
pub struct Driver {
    inner: Arc<DriverInner>,
}

// Contains the state associated with a `Driver`.
//
// This state may be shared between multiple `Driver` handles representing the same underlying
// driver. Only when the last `Driver` is dropped will the `Drop` implementation for this type run
// and the necessary driver resources will be de-allocated and unloaded.
//
// The same could be achieved by returning an `Arc<Driver>` from the `Host::load_driver` API,
// however the `DriverInner` abstraction is required in order to allow for the `Driver::destroy`
// method to exist safely. By wrapping the `Arc<DriverInner>` in the `Driver` type, we can make
// sure the user doesn't `try_unwrap` the `Arc` and invalidate the `Asio` instance's weak pointer.
// This would allow for instantiation of a separate driver before the existing one is destroyed,
// which is disallowed by ASIO.
#[derive(Debug)]
struct DriverInner {
    state: Mutex<DriverState>,
    // The unique name associated with this driver.
    name: String,
    // Track whether or not the driver has been destroyed.
    //
    // This allows for the user to manually destroy the driver and handle any errors if they wish.
    //
    // In the case that the driver has been manually destroyed this flag will be set to `true`
    // indicating to the `drop` implementation that there is nothing to be done.
    destroyed: bool,
}

/// All possible states of an ASIO `Driver` instance.
///
/// Mapped to the finite state machine in the ASIO SDK docs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DriverState {
    Initialized,
    Prepared,
    Running,
}

/// Amount of input and output
/// channels available.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Channels {
    pub ins: c_long,
    pub outs: c_long,
}

/// Sample rate of the ASIO driver.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SampleRate {
    pub rate: u32,
}

/// Holds the pointer to the callbacks that come from cpal
struct BufferCallback(Box<FnMut(i32) + Send>);

/// Input and Output streams.
///
/// There is only ever max one input and one output.
///
/// Only one is required.
pub struct AsioStreams {
    pub input: Option<AsioStream>,
    pub output: Option<AsioStream>,
}

/// A stream to ASIO.
///
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

lazy_static! {
    /// A global way to access all the callbacks.
    /// This is required because of how ASIO
    /// calls the buffer_switch function.
    /// Options are used so that when a callback is
    /// removed we don't change the Vec indicies.
    /// The indicies are how we match a callback
    /// with a stream.
    static ref BUFFER_CALLBACK: Mutex<Vec<Option<BufferCallback>>> = Mutex::new(Vec::new());
}

impl Asio {
    /// Initialise the ASIO API.
    pub fn new() -> Self {
        let loaded_driver = Mutex::new(Weak::new());
        Asio { loaded_driver }
    }

    /// Returns the name for each available driver.
    ///
    /// This is used at the start to allow the user to choose which driver they want.
    pub fn driver_names(&self) -> Vec<String> {
        // The most drivers we can take
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
                    let name = CStr::from_ptr(p_driver_name[i as usize]);
                    let my_driver_name = name.to_owned();
                    my_driver_name
                        .into_string()
                        .expect("Failed to convert driver name")
                }).collect()
        }
    }

    /// If a driver has already been loaded, this will return that driver.
    ///
    /// Returns `None` if no driver is currently loaded.
    ///
    /// This can be useful to check before calling `load_driver` as ASIO only supports loading a
    /// single driver at a time.
    pub fn loaded_driver(&self) -> Option<Driver> {
        self.loaded_driver
            .lock()
            .expect("failed to acquire loaded driver lock")
            .upgrade()
            .map(|inner| Driver { inner })
    }

    /// Load a driver from the given name.
    ///
    /// Driver names compatible with this method can be produced via the `asio.driver_names()`
    /// method.
    ///
    /// NOTE: Despite many requests from users, ASIO only supports loading a single driver at a
    /// time. Calling this method while a previously loaded `Driver` instance exists will result in
    /// an error. That said, if this method is called with the name of a driver that has already
    /// been loaded, that driver will be returned successfully.
    pub fn load_driver(&self, driver_name: &str) -> Result<Driver, LoadDriverError> {
        // Check whether or not a driver is already loaded.
        if let Some(driver) = self.loaded_driver() {
            if driver.name() == driver_name {
                return Ok(driver);
            } else {
                return Err(LoadDriverError::DriverAlreadyExists);
            }
        }

        // Make owned CString to send to load driver
        let driver_name_cstring = CString::new(driver_name)
            .expect("failed to create `CString` from driver name");
        let mut driver_info = ai::ASIODriverInfo {
            _bindgen_opaque_blob: [0u32; 43],
        };

        unsafe {
            // TODO: Check that a driver of the same name does not already exist?
            match ai::load_asio_driver(driver_name_cstring.as_ptr() as *mut i8) {
                false => Err(LoadDriverError::LoadDriverFailed),
                true => {
                    // Initialize ASIO.
                    asio_result!(ai::ASIOInit(&mut driver_info))?;
                    let state = Mutex::new(DriverState::Initialized);
                    let name = driver_name.to_string();
                    let destroyed = false;
                    let inner = Arc::new(DriverInner { name, state, destroyed });
                    *self.loaded_driver.lock().expect("failed to acquire loaded driver lock") =
                        Arc::downgrade(&inner);
                    let driver = Driver { inner };
                    Ok(driver)
                }
            }
        }
    }
}

impl BufferCallback {
    /// Calls the inner callback
    fn run(&mut self, index: i32) {
        let cb = &mut self.0;
        cb(index);
    }
}

impl Driver {
    /// The name used to uniquely identify this driver.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Returns the number of input and output channels available on the driver.
    pub fn channels(&self) -> Result<Channels, AsioError> {
        let mut ins: c_long = 0;
        let mut outs: c_long = 0;
        unsafe {
            asio_result!(ai::ASIOGetChannels(&mut ins, &mut outs))?;
        }
        let channel = Channels { ins, outs };
        Ok(channel)
    }

    /// Get current sample rate of the driver.
    pub fn sample_rate(&self) -> Result<c_double, AsioError> {
        let mut rate: c_double = 0.0;
        unsafe {
            asio_result!(ai::get_sample_rate(&mut rate))?;
        }
        Ok(rate)
    }

    /// Can the driver accept the given sample rate.
    pub fn can_sample_rate(&self, sample_rate: c_double) -> Result<bool, AsioError> {
        unsafe {
            match asio_result!(ai::can_sample_rate(sample_rate)) {
                Ok(()) => Ok(true),
                Err(AsioError::NoRate) => Ok(false),
                Err(err) => Err(err),
            }
        }
    }

    /// Set the sample rate for the driver.
    pub fn set_sample_rate(&self, sample_rate: c_double) -> Result<(), AsioError> {
        unsafe {
            asio_result!(ai::set_sample_rate(sample_rate))?;
        }
        Ok(())
    }

    /// Get the current data type of the driver.
    ///
    /// This queries a single channel's type assuming all channels have the same sample type.
    ///
    /// TODO: Make this a seperate call for input and output as it is possible that input and
    /// output have different sample types Initialize memory for calls.
    pub fn data_type(&self) -> Result<AsioSampleType, AsioError> {
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
            asio_result!(ai::ASIOGetChannelInfo(&mut channel_info))?;
            Ok(FromPrimitive::from_i32(channel_info.type_).expect("failed to cast sample type"))
        }
    }

    /// Ask ASIO to allocate the buffers and give the callback pointers.
    ///
    /// This will destroy any already allocated buffers.
    ///
    /// The prefered buffer size from ASIO is used.
    fn create_buffers(&self, buffer_infos: &mut [AsioBufferInfo]) -> Result<c_long, AsioError> {
        let num_channels = buffer_infos.len();
        let mut callbacks = AsioCallbacks {
            buffer_switch: buffer_switch,
            sample_rate_did_change: sample_rate_did_change,
            asio_message: asio_message,
            buffer_switch_time_info: buffer_switch_time_info,
        };
        // To pass as ai::ASIOCallbacks
        let callbacks: *mut _ = &mut callbacks;
        let mut min_b_size: c_long = 0;
        let mut max_b_size: c_long = 0;
        let mut pref_b_size: c_long = 0;
        let mut grans: c_long = 0;

        unsafe {
            // Get the buffer sizes
            // min possilbe size
            // max possible size
            // preferred size
            // granularity
            asio_result!(ai::ASIOGetBufferSize(
                &mut min_b_size,
                &mut max_b_size,
                &mut pref_b_size,
                &mut grans,
            ))?;

            if pref_b_size <= 0 {
                panic!(
                    "`ASIOGetBufferSize` produced unusable preferred buffer size of {}",
                    pref_b_size,
                );
            }

            if let DriverState::Running = self.inner.state() {
                self.stop()?;
            }
            if let DriverState::Prepared = self.inner.state() {
                self.dispose_buffers()?;
            }

            asio_result!(ai::ASIOCreateBuffers(
                buffer_infos.as_mut_ptr() as *mut _,
                num_channels as i32,
                pref_b_size,
                callbacks as *mut _,
            ))?;
        }
        self.inner.set_state(DriverState::Prepared);
        Ok(pref_b_size)
    }

    /// Creates the streams.
    ///
    /// Both input and output streams need to be created together as a single slice of
    /// `ASIOBufferInfo`.
    fn create_streams(&self, streams: AsioStreams) -> Result<AsioStreams, AsioError> {
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
                self.create_buffers(&mut bi).map(|buffer_size| {
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
            (Some(mut input), None) => {
                self.create_buffers(&mut input.buffer_infos)
                    .map(|buffer_size| {
                        AsioStreams {
                            input: Some(AsioStream {
                                buffer_infos: input.buffer_infos,
                                buffer_size,
                            }),
                            output: None,
                        }
                    })
            },
            // Just output
            (None, Some(mut output)) => {
                self.create_buffers(&mut output.buffer_infos)
                    .map(|buffer_size| {
                        AsioStreams {
                            output: Some(AsioStream {
                                buffer_infos: output.buffer_infos,
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

    /// Prepare the input stream.
    ///
    /// Because only the latest call to ASIOCreateBuffers is relevant this call will destroy all
    /// past active buffers and recreate them.
    ///
    /// For this reason we take the output stream if it exists.
    ///
    /// `num_channels` is the desired number of input channels.
    ///
    /// This returns a full AsioStreams with both input
    /// and output if output was active.
    pub fn prepare_input_stream(
        &self,
        output: Option<AsioStream>,
        num_channels: usize,
    ) -> Result<AsioStreams, AsioError> {
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
        &self,
        input: Option<AsioStream>,
        num_channels: usize,
    ) -> Result<AsioStreams, AsioError> {
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

    /// Releases buffers allocations.
    ///
    /// This will `stop` the stream if the driver is `Running`.
    ///
    /// No-op if no buffers are allocated.
    pub fn dispose_buffers(&self) -> Result<(), AsioError> {
        self.inner.dispose_buffers_inner()
    }

    /// Starts ASIO streams playing.
    ///
    /// The driver must be in the `Prepared` state
    ///
    /// If called successfully, the driver will be in the `Running` state.
    ///
    /// No-op if already `Running`.
    pub fn start(&self) -> Result<(), AsioError> {
        if let DriverState::Running = self.inner.state() {
            return Ok(());
        }
        unsafe {
            asio_result!(ai::ASIOStart())?;
        }
        self.inner.set_state(DriverState::Running);
        Ok(())
    }

    /// Stops ASIO streams playing.
    ///
    /// No-op if the state is not `Running`.
    ///
    /// If the state was `Running` and the stream is stopped successfully, the driver will be in
    /// the `Prepared` state.
    pub fn stop(&self) -> Result<(), AsioError> {
        self.inner.stop_inner()
    }

    /// Adds a callback to the list of active callbacks.
    ///
    /// The given function receives the index of the buffer currently ready for processing.
    pub fn set_callback<F>(&self, callback: F)
    where
        F: 'static + FnMut(i32) + Send,
    {
        let mut bc = BUFFER_CALLBACK.lock().unwrap();
        bc.push(Some(BufferCallback(Box::new(callback))));
    }

    /// Consumes and destroys the `Driver`, stopping the streams if they are running and releasing
    /// any associated resources.
    ///
    /// Returns `Ok(true)` if the driver was successfully destroyed.
    ///
    /// Returns `Ok(false)` if the driver was not destroyed because another handle to the driver
    /// still exists.
    ///
    /// Returns `Err` if some switching driver states failed or if ASIO returned an error on exit.
    pub fn destroy(self) -> Result<bool, AsioError> {
        let Driver { inner } = self;
        match Arc::try_unwrap(inner) {
            Err(_) => Ok(false),
            Ok(mut inner) => {
                inner.destroy_inner()?;
                Ok(true)
            }
        }
    }
}

impl DriverInner {
    fn state(&self) -> DriverState {
        *self.state.lock().expect("failed to lock `DriverState`")
    }

    fn set_state(&self, state: DriverState) {
        *self.state.lock().expect("failed to lock `DriverState`") = state;
    }

    fn stop_inner(&self) -> Result<(), AsioError> {
        if let DriverState::Running = self.state() {
            unsafe {
                asio_result!(ai::ASIOStop())?;
            }
            self.set_state(DriverState::Prepared);
        }
        Ok(())
    }

    fn dispose_buffers_inner(&self) -> Result<(), AsioError> {
        if let DriverState::Initialized = self.state() {
            return Ok(());
        }
        if let DriverState::Running = self.state() {
            self.stop_inner()?;
        }
        unsafe {
            asio_result!(ai::ASIODisposeBuffers())?;
        }
        self.set_state(DriverState::Initialized);
        Ok(())
    }

    fn destroy_inner(&mut self) -> Result<(), AsioError> {
        // Drop back through the driver state machine one state at a time.
        if let DriverState::Running = self.state() {
            self.stop_inner()?;
        }
        if let DriverState::Prepared = self.state() {
            self.dispose_buffers_inner()?;
        }
        unsafe {
            asio_result!(ai::ASIOExit())?;
            ai::remove_current_driver();
        }

        // Clear any existing stream callbacks.
        if let Ok(mut bcs) = BUFFER_CALLBACK.lock() {
            bcs.clear();
        }

        // Signal that the driver has been destroyed.
        self.destroyed = true;

        Ok(())
    }
}

impl Drop for DriverInner {
    fn drop(&mut self) {
        if self.destroyed {
            // We probably shouldn't `panic!` in the destructor? We also shouldn't ignore errors
            // though either.
            self.destroy_inner().ok();
        }
    }
}

unsafe impl Send for AsioStream {}

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

/// This is called by ASIO.
/// Here we run the callback for each stream.
/// double_buffer_index is either 0 or 1
/// indicating which buffer to fill
extern "C" fn buffer_switch(double_buffer_index: c_long, _direct_process: c_long) -> () {
    // This lock is probably unavoidable
    // but locks in the audio stream is not great
    let mut bcs = BUFFER_CALLBACK.lock().unwrap();

    for mut bc in bcs.iter_mut() {
        if let Some(ref mut bc) = bc {
            bc.run(double_buffer_index);
        }
    }
}
