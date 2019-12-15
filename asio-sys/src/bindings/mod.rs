pub mod asio_import;
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

/// A rust-usable version of the `ASIOTime` type that does not contain a binary blob for fields.
#[repr(C)]
pub struct AsioTime {
    /// Must be `0`.
    pub reserved: [c_long; 4],
    /// Required.
    pub time_info: AsioTimeInfo,
    /// Optional, evaluated if (time_code.flags & ktcValid).
    pub time_code: AsioTimeCode,
}

/// A rust-compatible version of the `ASIOTimeInfo` type that does not contain a binary blob for
/// fields.
#[repr(C)]
pub struct AsioTimeInfo {
    /// Absolute speed (1. = nominal).
    pub speed: c_double,
    /// System time related to sample_position, in nanoseconds.
    ///
    /// On Windows, must be derived from timeGetTime().
    pub system_time: ai::ASIOTimeStamp,
    /// Sample position since `ASIOStart()`.
    pub sample_position: ai::ASIOSamples,
    /// Current rate, unsigned.
    pub sample_rate: AsioSampleRate,
    /// See `AsioTimeInfoFlags`.
    pub flags: c_long,
    /// Must be `0`.
    pub reserved: [c_char; 12],
}

/// A rust-compatible version of the `ASIOTimeCode` type that does not use a binary blob for its
/// fields.
#[repr(C)]
pub struct AsioTimeCode {
    /// Speed relation (fraction of nominal speed) optional.
    ///
    /// Set to 0. or 1. if not supported.
    pub speed: c_double,
    /// Time in samples unsigned.
    pub time_code_samples: ai::ASIOSamples,
    /// See `ASIOTimeCodeFlags`.
    pub flags: c_long,
    /// Set to `0`.
    pub future: [c_char; 64],
}

/// A rust-compatible version of the `ASIOSampleRate` type that does not use a binary blob for its
/// fields.
pub type AsioSampleRate = f64;

// A helper type to simplify retrieval of available buffer sizes.
#[derive(Default)]
struct BufferSizes {
    min: c_long,
    max: c_long,
    pref: c_long,
    grans: c_long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackId(usize);

lazy_static! {
    /// A global way to access all the callbacks.
    ///
    /// This is required because of how ASIO calls the `buffer_switch` function with no data
    /// parameters.
    ///
    /// Options are used so that when a callback is removed we don't change the Vec indices.
    ///
    /// The indices are how we match a callback with a stream.
    static ref BUFFER_CALLBACK: Mutex<Vec<(CallbackId, BufferCallback)>> = Mutex::new(Vec::new());
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
        const MAX_DRIVER_NAME_LEN: usize = 32;

        // 2D array of driver names set to 0.
        let mut driver_names: [[c_char; MAX_DRIVER_NAME_LEN]; MAX_DRIVERS] =
            [[0; MAX_DRIVER_NAME_LEN]; MAX_DRIVERS];
        // Pointer to each driver name.
        let mut driver_name_ptrs: [*mut i8; MAX_DRIVERS] = [0 as *mut i8; MAX_DRIVERS];
        for (ptr, name) in driver_name_ptrs.iter_mut().zip(&mut driver_names[..]) {
            *ptr = (*name).as_mut_ptr();
        }

        unsafe {
            let num_drivers = ai::get_driver_names(driver_name_ptrs.as_mut_ptr(), MAX_DRIVERS as i32);
            (0 .. num_drivers)
                .map(|i| driver_name_to_utf8(&driver_names[i as usize]).to_string())
                .collect()
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
    /// Calls the inner callback.
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

    /// Get the current data type of the driver's input stream.
    ///
    /// This queries a single channel's type assuming all channels have the same sample type.
    pub fn input_data_type(&self) -> Result<AsioSampleType, AsioError> {
        stream_data_type(true)
    }

    /// Get the current data type of the driver's output stream.
    ///
    /// This queries a single channel's type assuming all channels have the same sample type.
    pub fn output_data_type(&self) -> Result<AsioSampleType, AsioError> {
        stream_data_type(false)
    }

    /// Ask ASIO to allocate the buffers and give the callback pointers.
    ///
    /// This will destroy any already allocated buffers.
    ///
    /// The preferred buffer size from ASIO is used.
    fn create_buffers(&self, buffer_infos: &mut [AsioBufferInfo]) -> Result<c_long, AsioError> {
        let num_channels = buffer_infos.len();

        // To pass as ai::ASIOCallbacks
        let mut callbacks = create_asio_callbacks();

        // Retrieve the available buffer sizes.
        let buffer_sizes = asio_get_buffer_sizes()?;
        if buffer_sizes.pref <= 0 {
            panic!(
                "`ASIOGetBufferSize` produced unusable preferred buffer size of {}",
                buffer_sizes.pref,
            );
        }

        // Ensure the driver is in the `Initialized` state.
        if let DriverState::Running = self.inner.state() {
            self.stop()?;
        }
        if let DriverState::Prepared = self.inner.state() {
            self.dispose_buffers()?;
        }

        unsafe {
            asio_result!(ai::ASIOCreateBuffers(
                buffer_infos.as_mut_ptr() as *mut _,
                num_channels as i32,
                buffer_sizes.pref,
                &mut callbacks as *mut _ as *mut _,
            ))?;
        }

        self.inner.set_state(DriverState::Prepared);
        Ok(buffer_sizes.pref)
    }

    /// Creates the streams.
    ///
    /// Both input and output streams need to be created together as a single slice of
    /// `ASIOBufferInfo`.
    fn create_streams(
        &self,
        mut input_buffer_infos: Vec<AsioBufferInfo>,
        mut output_buffer_infos: Vec<AsioBufferInfo>,
    ) -> Result<AsioStreams, AsioError> {
        let (input, output) = match (input_buffer_infos.is_empty(), output_buffer_infos.is_empty()) {
            // Both stream exist.
            (false, false) => {
                // Create one continuous slice of buffers.
                let split_point = input_buffer_infos.len();
                let mut all_buffer_infos = input_buffer_infos;
                all_buffer_infos.append(&mut output_buffer_infos);
                // Create the buffers. On success, split the output and input again.
                let buffer_size = self.create_buffers(&mut all_buffer_infos)?;
                let output_buffer_infos = all_buffer_infos.split_off(split_point);
                let input_buffer_infos = all_buffer_infos;
                let input = Some(AsioStream {
                    buffer_infos: input_buffer_infos,
                    buffer_size,
                });
                let output = Some(AsioStream {
                    buffer_infos: output_buffer_infos,
                    buffer_size,
                });
                (input, output)
            },
            // Just input
            (false, true) => {
                let buffer_size = self.create_buffers(&mut input_buffer_infos)?;
                let input = Some(AsioStream {
                    buffer_infos: input_buffer_infos,
                    buffer_size,
                });
                let output = None;
                (input, output)
            },
            // Just output
            (true, false) => {
                let buffer_size = self.create_buffers(&mut output_buffer_infos)?;
                let input = None;
                let output = Some(AsioStream {
                    buffer_infos: output_buffer_infos,
                    buffer_size,
                });
                (input, output)
            },
            // Impossible
            (true, true) => unreachable!("Trying to create streams without preparing"),
        };
        Ok(AsioStreams { input, output })
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
    /// This returns a full AsioStreams with both input and output if output was active.
    pub fn prepare_input_stream(
        &self,
        output: Option<AsioStream>,
        num_channels: usize,
    ) -> Result<AsioStreams, AsioError> {
        let input_buffer_infos = prepare_buffer_infos(true, num_channels);
        let output_buffer_infos = output
            .map(|output| output.buffer_infos)
            .unwrap_or_else(Vec::new);
        self.create_streams(input_buffer_infos, output_buffer_infos)
    }

    /// Prepare the output stream.
    ///
    /// Because only the latest call to ASIOCreateBuffers is relevant this call will destroy all
    /// past active buffers and recreate them.
    ///
    /// For this reason we take the input stream if it exists.
    ///
    /// `num_channels` is the desired number of output channels.
    ///
    /// This returns a full AsioStreams with both input and output if input was active.
    pub fn prepare_output_stream(
        &self,
        input: Option<AsioStream>,
        num_channels: usize,
    ) -> Result<AsioStreams, AsioError> {
        let input_buffer_infos = input
            .map(|input| input.buffer_infos)
            .unwrap_or_else(Vec::new);
        let output_buffer_infos = prepare_buffer_infos(false, num_channels);
        self.create_streams(input_buffer_infos, output_buffer_infos)
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
    ///
    /// Returns an ID uniquely associated with the given callback so that it may be removed later.
    pub fn add_callback<F>(&self, callback: F) -> CallbackId
    where
        F: 'static + FnMut(i32) + Send,
    {
        let mut bc = BUFFER_CALLBACK.lock().unwrap();
        let id = bc
            .last()
            .map(|&(id, _)| CallbackId(id.0.checked_add(1).expect("stream ID overflowed")))
            .unwrap_or(CallbackId(0));
        let cb = BufferCallback(Box::new(callback));
        bc.push((id, cb));
        id
    }

    /// Remove the callback with the given ID.
    pub fn remove_callback(&self, rem_id: CallbackId) {
        let mut bc = BUFFER_CALLBACK.lock().unwrap();
        bc.retain(|&(id, _)| id != rem_id);
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
        if !self.destroyed {
            // We probably shouldn't `panic!` in the destructor? We also shouldn't ignore errors
            // though either.
            self.destroy_inner().ok();
        }
    }
}

unsafe impl Send for AsioStream {}

/// Used by the input and output stream creation process.
fn prepare_buffer_infos(is_input: bool, n_channels: usize) -> Vec<AsioBufferInfo> {
    let is_input = if is_input { 1 } else { 0 };
    (0..n_channels)
        .map(|ch| {
            let channel_num = ch as c_long;
            // To be filled by ASIOCreateBuffers.
            let buffers = [std::ptr::null_mut(); 2];
            AsioBufferInfo { is_input, channel_num, buffers }
        })
        .collect()
}

/// The set of callbacks passed to `ASIOCreateBuffers`.
fn create_asio_callbacks() -> AsioCallbacks {
    AsioCallbacks {
        buffer_switch: buffer_switch,
        sample_rate_did_change: sample_rate_did_change,
        asio_message: asio_message,
        buffer_switch_time_info: buffer_switch_time_info,
    }
}

/// Retrieve the minimum, maximum and preferred buffer sizes along with the available
/// buffer size granularity.
fn asio_get_buffer_sizes() -> Result<BufferSizes, AsioError> {
    let mut b = BufferSizes::default();
    unsafe {
        let res = ai::ASIOGetBufferSize(&mut b.min, &mut b.max, &mut b.pref, &mut b.grans);
        asio_result!(res)?;
    }
    Ok(b)
}

/// Retrieve the `ASIOChannelInfo` associated with the channel at the given index on either the
/// input or output stream (`true` for input).
fn asio_channel_info(channel: c_long, is_input: bool) -> Result<ai::ASIOChannelInfo, AsioError> {
    let mut channel_info = ai::ASIOChannelInfo {
        // Which channel we are querying
        channel,
        // Was it input or output
        isInput: if is_input { 1 } else { 0 },
        // Was it active
        isActive: 0,
        channelGroup: 0,
        // The sample type
        type_: 0,
        name: [0 as c_char; 32],
    };
    unsafe {
        asio_result!(ai::ASIOGetChannelInfo(&mut channel_info))?;
        Ok(channel_info)
    }
}

/// Retrieve the data type of either the input or output stream.
///
/// If `is_input` is true, this will be queried on the input stream.
fn stream_data_type(is_input: bool) -> Result<AsioSampleType, AsioError> {
    let channel_info = asio_channel_info(0, is_input)?;
    Ok(FromPrimitive::from_i32(channel_info.type_).expect("unkown `ASIOSampletype` value"))
}

/// ASIO uses null terminated c strings for driver names.
///
/// This converts to utf8.
fn driver_name_to_utf8(bytes: &[c_char]) -> std::borrow::Cow<str> {
    unsafe {
        CStr::from_ptr(bytes.as_ptr()).to_string_lossy()
    }
}

/// ASIO uses null terminated c strings for channel names.
///
/// This converts to utf8.
fn _channel_name_to_utf8(bytes: &[c_char]) -> std::borrow::Cow<str> {
    unsafe {
        CStr::from_ptr(bytes.as_ptr()).to_string_lossy()
    }
}

/// Indicates the stream sample rate has changed.
///
/// TODO: Provide some way of allowing CPAL to handle this.
extern "C" fn sample_rate_did_change(s_rate: c_double) -> () {
    eprintln!("unhandled sample rate change to {}", s_rate);
}

/// Message callback for ASIO to notify of certain events.
extern "C" fn asio_message(
    selector: c_long,
    value: c_long,
    _message: *mut (),
    _opt: *mut c_double,
) -> c_long {
    match selector {
        ai::kAsioSelectorSupported => {
            // Indicate what message selectors are supported.
            match value {
                | ai::kAsioResetRequest
                | ai::kAsioEngineVersion
                | ai::kAsioResyncRequest
                | ai::kAsioLatenciesChanged
                // Following added in ASIO 2.0.
                | ai::kAsioSupportsTimeInfo
                | ai::kAsioSupportsTimeCode
                | ai::kAsioSupportsInputMonitor => 1,
                _ => 0,
            }
        }

        ai::kAsioResetRequest => {
            // Defer the task and perform the reset of the driver during the next "safe" situation
            // You cannot reset the driver right now, as this code is called from the driver. Reset
            // the driver is done by completely destruct it. I.e. ASIOStop(), ASIODisposeBuffers(),
            // Destruction. Afterwards you initialize the driver again.
            // TODO: Handle this.
            1
        }

        ai::kAsioResyncRequest => {
            // This informs the application, that the driver encountered some non fatal data loss.
            // It is used for synchronization purposes of different media. Added mainly to work
            // around the Win16Mutex problems in Windows 95/98 with the Windows Multimedia system,
            // which could loose data because the Mutex was hold too long by another thread.
            // However a driver can issue it in other situations, too.
            // TODO: Handle this.
            1
        }

        ai::kAsioLatenciesChanged => {
            // This will inform the host application that the drivers were latencies changed.
            // Beware, it this does not mean that the buffer sizes have changed! You might need to
            // update internal delay data.
            // TODO: Handle this.
            1
        }

        ai::kAsioEngineVersion => {
            // Return the supported ASIO version of the host application If a host applications
            // does not implement this selector, ASIO 1.0 is assumed by the driver
            2
        }

        ai::kAsioSupportsTimeInfo => {
            // Informs the driver whether the asioCallbacks.bufferSwitchTimeInfo() callback is
            // supported. For compatibility with ASIO 1.0 drivers the host application should
            // always support the "old" bufferSwitch method, too, which we do.
            1
        }

        ai::kAsioSupportsTimeCode => {
            // Informs the driver whether the application is interested in time code info. If an
            // application does not need to know about time code, the driver has less work to do.
            // TODO: Provide an option for this?
            0
        }

        _ => 0, // Unknown/unhandled message type.
    }
}

/// Similar to buffer switch but with time info.
///
/// If only `buffer_switch` is called by the driver instead, the `buffer_switch` callback will
/// create the necessary timing info and call this function.
///
/// TODO: Provide some access to `ai::ASIOTime` once CPAL gains support for time stamps.
extern "C" fn buffer_switch_time_info(
    time: *mut ai::ASIOTime,
    double_buffer_index: c_long,
    _direct_process: c_long,
) -> *mut ai::ASIOTime {
    // This lock is probably unavoidable, but locks in the audio stream are not great.
    let mut bcs = BUFFER_CALLBACK.lock().unwrap();
    for &mut (_, ref mut bc) in bcs.iter_mut() {
        bc.run(double_buffer_index);
    }
    time
}

/// This is called by ASIO.
///
/// Here we run the callback for each stream.
///
/// `double_buffer_index` is either `0` or `1`  indicating which buffer to fill.
extern "C" fn buffer_switch(double_buffer_index: c_long, direct_process: c_long) -> () {
    // Emulate the time info provided by the `buffer_switch_time_info` callback.
    // This is an attempt at matching the behaviour in `hostsample.cpp` from the SDK.
    let mut time = unsafe {
        let mut time: AsioTime = std::mem::zeroed();
        let res = ai::ASIOGetSamplePosition(
            &mut time.time_info.sample_position,
            &mut time.time_info.system_time,
        );
        if let Ok(()) = asio_result!(res) {
            time.time_info.flags =
                (ai::AsioTimeInfoFlags::kSystemTimeValid | ai::AsioTimeInfoFlags::kSamplePositionValid).0;
        }
        time
    };

    // Actual processing happens within the `buffer_switch_time_info` callback.
    let asio_time_ptr = &mut time as *mut AsioTime as *mut ai::ASIOTime;
    buffer_switch_time_info(asio_time_ptr, double_buffer_index, direct_process);
}

#[test]
fn check_type_sizes() {
    assert_eq!(std::mem::size_of::<AsioSampleRate>(), std::mem::size_of::<ai::ASIOSampleRate>());
    assert_eq!(std::mem::size_of::<AsioTimeCode>(), std::mem::size_of::<ai::ASIOTimeCode>());
    assert_eq!(std::mem::size_of::<AsioTime>(), std::mem::size_of::<ai::ASIOTime>());
}
