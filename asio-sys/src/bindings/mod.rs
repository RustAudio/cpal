pub(crate) mod asio_import;
#[macro_use]
pub mod errors;

use self::errors::{AsioError, AsioErrorWrapper, LoadDriverError};
use num_traits::FromPrimitive;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_void};
use std::ptr::null_mut;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc, Mutex, MutexGuard, Weak,
};
use std::time::Duration;

// On Windows (where ASIO actually runs), c_long is i32.
// On non-Windows platforms (for docs.rs and local testing), redefine c_long as i32 to match.
#[cfg(target_os = "windows")]
use std::os::raw::c_long;
#[cfg(not(target_os = "windows"))]
type c_long = i32;

// Bindings import
use self::asio_import as ai;

/// A handle to the ASIO API.
///
/// There should only be one instance of this type at any point in time.
#[derive(Debug, Default)]
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
pub(crate) enum DriverState {
    Initialized,
    Prepared,
    Running,
}

/// Amount of input and output channels available.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Channels {
    pub ins: i32,
    pub outs: i32,
}

/// Hardware latency in frames for the input and output streams.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Latencies {
    pub input: i32,
    pub output: i32,
}

/// Minimum and maximum supported buffer sizes in frames.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BufferSizeRange {
    pub min: i32,
    pub max: i32,
}

/// Information provided to the BufferCallback.
#[derive(Debug)]
pub struct CallbackInfo {
    pub buffer_index: i32,
    /// System time at the start of this buffer period, in nanoseconds.
    pub system_time: u64,
    pub callback_flag: u32,
}

/// Holds the pointer to the callbacks that come from cpal
struct BufferCallback(Box<dyn FnMut(&CallbackInfo) + Send>);

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
#[repr(C, packed(4))]
pub struct AsioBufferInfo {
    /// 0 for output 1 for input
    pub is_input: i32,
    /// Which channel. Starts at 0
    pub channel_num: i32,
    /// Pointer to each half of the double buffer.
    pub buffers: [*mut c_void; 2],
}

/// Callbacks that ASIO calls
#[repr(C, packed(4))]
struct AsioCallbacks {
    buffer_switch: extern "C" fn(double_buffer_index: c_long, direct_process: c_long) -> (),
    sample_rate_did_change: extern "C" fn(s_rate: c_double) -> (),
    asio_message: extern "C" fn(
        selector: c_long,
        value: c_long,
        message: *mut (),
        opt: *mut c_double,
    ) -> c_long,
    buffer_switch_time_info: extern "C" fn(
        params: *mut ai::ASIOTime,
        double_buffer_index: c_long,
        direct_process: c_long,
    ) -> *mut ai::ASIOTime,
}

static ASIO_CALLBACKS: AsioCallbacks = AsioCallbacks {
    buffer_switch,
    sample_rate_did_change,
    asio_message,
    buffer_switch_time_info,
};

/// All the possible types from ASIO.
/// This is a direct copy of the asioMessage selectors
/// inside ASIO SDK.
#[rustfmt::skip]
#[derive(Clone, Copy, Debug, FromPrimitive)]
#[repr(C)]
pub enum AsioMessageSelectors {
    kAsioSelectorSupported = 1, // selector in <value>, returns 1L if supported,
                                // 0 otherwise
    kAsioEngineVersion,         // returns engine (host) asio implementation version,
                                // 2 or higher
    kAsioResetRequest,          // request driver reset. if accepted, this
                                // will close the driver (ASIO_Exit() ) and
                                // re-open it again (ASIO_Init() etc). some
                                // drivers need to reconfigure for instance
                                // when the sample rate changes, or some basic
                                // changes have been made in ASIO_ControlPanel().
                                // returns 1L; note the request is merely passed
                                // to the application, there is no way to determine
                                // if it gets accepted at this time (but it usually
                                // will be).
    kAsioBufferSizeChange,      // not yet supported, will currently always return 0L.
                                // for now, use kAsioResetRequest instead.
                                // once implemented, the new buffer size is expected
                                // in <value>, and on success returns 1L
    kAsioResyncRequest,         // the driver went out of sync, such that
                                // the timestamp is no longer valid. this
                                // is a request to re-start the engine and
                                // slave devices (sequencer). returns 1 for ok,
                                // 0 if not supported.
    kAsioLatenciesChanged,      // the drivers latencies have changed. The engine
                                // will refetch the latencies.
    kAsioSupportsTimeInfo,      // if host returns true here, it will expect the
                                // callback bufferSwitchTimeInfo to be called instead
                                // of bufferSwitch
    kAsioSupportsTimeCode,      //
    kAsioMMCCommand,            // unused - value: number of commands, message points to mmc commands
    kAsioSupportsInputMonitor,  // kAsioSupportsXXX return 1 if host supports this
    kAsioSupportsInputGain,     // unused and undefined
    kAsioSupportsInputMeter,    // unused and undefined
    kAsioSupportsOutputGain,    // unused and undefined
    kAsioSupportsOutputMeter,   // unused and undefined
    kAsioOverload,              // driver detected an overload
    kAsioNumMessageSelectors,   // sentinel value equal to the number of defined selectors
}

/// Events dispatched to registered driver event callbacks.
#[derive(Clone, Copy, Debug)]
pub enum AsioDriverEvent {
    /// A message from the ASIO driver's `asioMessage` callback.
    ///
    /// `selector` identifies the message type; `value` is the raw payload passed by the driver.
    /// For [`AsioMessageSelectors::kAsioSelectorSupported`] queries, `value` is the selector being
    /// queried. Return `true` to advertise support for it, `false` to decline. For other selectors,
    /// the return value is ignored.
    Message {
        selector: AsioMessageSelectors,
        value: i32,
    },

    /// The ASIO driver reported a sample rate change.
    ///
    /// Only dispatched when the reported rate differs from the last known rate, so spurious
    /// `sampleRateDidChange` calls (e.g. on AES/EBU sync status changes where the rate has not
    /// actually changed) are suppressed.
    SampleRateChanged(f64),
}

/// A rust-usable version of the `ASIOTime` type that does not contain a binary blob for fields.
#[repr(C, packed(4))]
pub struct AsioTime {
    /// Must be `0`.
    reserved: [i32; 4],
    /// Required.
    pub time_info: AsioTimeInfo,
    /// Optional, evaluated if (time_code.flags & ktcValid).
    pub time_code: AsioTimeCode,
}

/// A rust-compatible version of the `ASIOTimeInfo` type that does not contain a binary blob for
/// fields.
#[repr(C, packed(4))]
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
    pub flags: i32,
    /// Must be `0`.
    reserved: [c_char; 12],
}

/// A rust-compatible version of the `ASIOTimeCode` type that does not use a binary blob for its
/// fields.
#[repr(C, packed(4))]
pub struct AsioTimeCode {
    /// Speed relation (fraction of nominal speed) optional.
    ///
    /// Set to 0. or 1. if not supported.
    pub speed: c_double,
    /// Time in samples unsigned.
    pub time_code_samples: ai::ASIOSamples,
    /// See `ASIOTimeCodeFlags`.
    pub flags: i32,
    /// Set to `0`.
    future: [c_char; 64],
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

/// Identifies a buffer callback registered via [`Driver::add_callback`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferCallbackId(usize);

/// A global way to access all the callbacks.
///
/// This is required because of how ASIO calls the `buffer_switch` function with no data
/// parameters.
static BUFFER_CALLBACK: Mutex<Vec<(BufferCallbackId, BufferCallback)>> = Mutex::new(Vec::new());

/// Used to identify when to clear buffers.
static CALLBACK_FLAG: AtomicU32 = AtomicU32::new(0);

/// Indicates that ASIOOutputReady should be called
static CALL_OUTPUT_READY: AtomicBool = AtomicBool::new(false);
static CURRENT_SAMPLE_RATE: AtomicU64 = AtomicU64::new(0);

/// Identifies a driver event callback registered via [`Driver::add_event_callback`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverEventCallbackId(usize);

struct DriverEventCallback(Arc<dyn Fn(AsioDriverEvent) -> bool + Send + Sync>);

/// A global registry for ASIO driver event callbacks.
static DRIVER_EVENT_CALLBACKS: Mutex<Vec<(DriverEventCallbackId, DriverEventCallback)>> =
    Mutex::new(Vec::new());

impl Asio {
    /// Initialise the ASIO API.
    pub fn new() -> Self {
        Self::default()
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
        let mut driver_name_ptrs: [*mut i8; MAX_DRIVERS] = [null_mut(); MAX_DRIVERS];
        for (ptr, name) in driver_name_ptrs.iter_mut().zip(&mut driver_names[..]) {
            *ptr = (*name).as_mut_ptr();
        }

        unsafe {
            let num_drivers =
                ai::get_driver_names(driver_name_ptrs.as_mut_ptr(), MAX_DRIVERS as i32);
            (0..num_drivers)
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
        // Hold the lock for the entire operation to prevent a TOCTOU race where two threads
        // both pass the "no driver loaded" check and then both call load_asio_driver.
        let mut loaded = self
            .loaded_driver
            .lock()
            .expect("failed to acquire loaded driver lock");

        // Check whether or not a driver is already loaded.
        if let Some(inner) = loaded.upgrade() {
            let driver = Driver { inner };
            if driver.name() == driver_name {
                return Ok(driver);
            } else {
                return Err(LoadDriverError::DriverAlreadyExists);
            }
        }

        // Make owned CString to send to load driver
        let driver_name_cstring =
            CString::new(driver_name).map_err(|_| LoadDriverError::LoadDriverFailed)?;
        let mut driver_info = std::mem::MaybeUninit::<ai::ASIODriverInfo>::uninit();

        unsafe {
            match ai::load_asio_driver(driver_name_cstring.as_ptr() as *mut i8) {
                false => Err(LoadDriverError::LoadDriverFailed),
                true => {
                    // Initialize ASIO.
                    asio_result!(ai::ASIOInit(driver_info.as_mut_ptr()))?;
                    let _driver_info = driver_info.assume_init();
                    let mut rate: c_double = 0.0;
                    let _ = asio_result!(ai::get_sample_rate(&mut rate));
                    if rate > 0.0 {
                        CURRENT_SAMPLE_RATE.store(rate.to_bits(), Ordering::Release);
                    }
                    let state = Mutex::new(DriverState::Initialized);
                    let name = driver_name.to_string();
                    let destroyed = false;
                    let inner = Arc::new(DriverInner {
                        name,
                        state,
                        destroyed,
                    });
                    *loaded = Arc::downgrade(&inner);
                    let driver = Driver { inner };
                    Ok(driver)
                }
            }
        }
    }
}

impl BufferCallback {
    /// Calls the inner callback.
    fn run(&mut self, callback_info: &CallbackInfo) {
        let cb = &mut self.0;
        cb(callback_info);
    }
}

impl Driver {
    /// The name used to uniquely identify this driver.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Returns the number of input and output channels available on the driver.
    pub fn channels(&self) -> Result<Channels, AsioError> {
        let _guard = self.inner.lock_state();
        let mut ins: c_long = 0;
        let mut outs: c_long = 0;
        unsafe {
            asio_result!(ai::ASIOGetChannels(&mut ins, &mut outs))?;
        }
        Ok(Channels { ins, outs })
    }

    /// Get the input and output hardware latency in frames.
    pub fn latencies(&self) -> Result<Latencies, AsioError> {
        let _guard = self.inner.lock_state();
        let mut input_latency: c_long = 0;
        let mut output_latency: c_long = 0;
        unsafe {
            asio_result!(ai::ASIOGetLatencies(
                &mut input_latency,
                &mut output_latency
            ))?;
        }
        Ok(Latencies {
            input: input_latency,
            output: output_latency,
        })
    }

    /// Get the min and max supported buffersize of the driver.
    pub fn buffersize_range(&self) -> Result<BufferSizeRange, AsioError> {
        let _guard = self.inner.lock_state();
        let buffer_sizes = asio_get_buffer_sizes()?;
        Ok(BufferSizeRange {
            min: buffer_sizes.min,
            max: buffer_sizes.max,
        })
    }

    /// Get current sample rate of the driver.
    pub fn sample_rate(&self) -> Result<f64, AsioError> {
        let _guard = self.inner.lock_state();
        let mut rate: c_double = 0.0;
        unsafe {
            asio_result!(ai::get_sample_rate(&mut rate))?;
        }
        Ok(rate)
    }

    /// Can the driver accept the given sample rate.
    pub fn can_sample_rate(&self, sample_rate: f64) -> Result<bool, AsioError> {
        let _guard = self.inner.lock_state();
        unsafe {
            match asio_result!(ai::can_sample_rate(sample_rate)) {
                Ok(()) => Ok(true),
                Err(AsioError::NoRate) => Ok(false),
                Err(err) => Err(err),
            }
        }
    }

    /// Set the sample rate for the driver.
    pub fn set_sample_rate(&self, sample_rate: f64) -> Result<(), AsioError> {
        let actual = {
            let _guard = self.inner.lock_state();
            unsafe { asio_result!(ai::set_sample_rate(sample_rate))? };
            let mut actual: c_double = 0.0;
            unsafe { asio_result!(ai::get_sample_rate(&mut actual))? };
            actual
        };

        // Check whether the driver applied the rate immediately.
        if (actual - sample_rate).abs() < 1.0 {
            CURRENT_SAMPLE_RATE.store(actual.to_bits(), Ordering::Release);
            return Ok(());
        }

        // Some ASIO drivers (e.g. Steinberg) do not apply a rate change until after a
        // complete buffer-creation cycle (CreateBuffers -> Start -> Stop -> DisposeBuffers),
        // followed by a full driver teardown and reload.
        let mut dummy_infos = prepare_buffer_infos(false, 1);
        let buffer_size = self.create_buffers(&mut dummy_infos, None)?;

        // Start briefly so the driver reconfigures its hardware clock.
        self.start()?;

        // Wait for one full buffer to be processed: this guarantees the driver has
        // applied the rate change to the hardware clock before we stop it.
        let buffer_duration = Duration::from_secs_f64(buffer_size as f64 / sample_rate);
        std::thread::sleep(buffer_duration);

        self.stop()?;
        self.dispose_buffers()?;

        // Full teardown so the driver is reset to a clean state. Some drivers
        // (e.g. Steinberg) return errors from ASIOGetChannels after DisposeBuffers
        // unless the driver is fully exited and reloaded.
        {
            let mut state = self.inner.lock_state();
            unsafe {
                let _ = asio_result!(ai::ASIOExit());
                ai::remove_current_driver();
            }
            std::thread::sleep(buffer_duration);

            // Safety: the name was validated as null-free when the driver was first loaded.
            let name_cstring = CString::new(self.inner.name.as_str())
                .expect("driver name already stored must not contain null bytes");
            unsafe {
                if !ai::load_asio_driver(name_cstring.as_ptr() as *mut i8) {
                    return Err(AsioError::NoDrivers);
                }
                let mut driver_info = std::mem::MaybeUninit::<ai::ASIODriverInfo>::uninit();
                asio_result!(ai::ASIOInit(driver_info.as_mut_ptr()))?;
            }
            *state = DriverState::Initialized;

            // Set the rate again on the freshly initialized driver.
            unsafe { asio_result!(ai::set_sample_rate(sample_rate))? };

            let mut actual: c_double = 0.0;
            unsafe { asio_result!(ai::get_sample_rate(&mut actual))? };
            if (actual - sample_rate).abs() >= 1.0 {
                return Err(AsioError::NoRate);
            }

            CURRENT_SAMPLE_RATE.store(actual.to_bits(), Ordering::Release);
        }
        Ok(())
    }

    /// Get the current data type of the driver's input stream.
    ///
    /// This queries a single channel's type assuming all channels have the same sample type.
    pub fn input_data_type(&self) -> Result<AsioSampleType, AsioError> {
        let _guard = self.inner.lock_state();
        stream_data_type(true)
    }

    /// Get the current data type of the driver's output stream.
    ///
    /// This queries a single channel's type assuming all channels have the same sample type.
    pub fn output_data_type(&self) -> Result<AsioSampleType, AsioError> {
        let _guard = self.inner.lock_state();
        stream_data_type(false)
    }

    /// Ask ASIO to allocate the buffers and give the callback pointers.
    ///
    /// This will destroy any already allocated buffers.
    ///
    /// If buffersize is None then the preferred buffer size from ASIO is used,
    /// otherwise the desired buffersize is used if the requested size is within
    /// the range of accepted buffersizes for the device.
    fn create_buffers(
        &self,
        buffer_infos: &mut [AsioBufferInfo],
        buffer_size: Option<i32>,
    ) -> Result<c_long, AsioError> {
        let num_channels = buffer_infos.len();

        let mut state = self.inner.lock_state();

        // Retrieve the available buffer sizes.
        let buffer_sizes = asio_get_buffer_sizes()?;
        if buffer_sizes.pref <= 0 {
            panic!(
                "`ASIOGetBufferSize` produced unusable preferred buffer size of {}",
                buffer_sizes.pref,
            );
        }

        let buffer_size = match buffer_size {
            Some(v) => {
                if v <= buffer_sizes.max {
                    v
                } else {
                    return Err(AsioError::InvalidBufferSize);
                }
            }
            None => buffer_sizes.pref,
        };

        CALL_OUTPUT_READY.store(
            asio_result!(unsafe { ai::ASIOOutputReady() }).is_ok(),
            Ordering::Release,
        );

        // Ensure the driver is in the `Initialized` state.
        if let DriverState::Running = *state {
            state.stop()?;
        }
        if let DriverState::Prepared = *state {
            state.dispose_buffers()?;
        }
        unsafe {
            asio_result!(ai::ASIOCreateBuffers(
                buffer_infos.as_mut_ptr() as *mut _,
                num_channels as i32,
                buffer_size,
                &ASIO_CALLBACKS as *const _ as *mut _,
            ))?;
        }
        *state = DriverState::Prepared;

        Ok(buffer_size)
    }

    /// Creates the streams.
    ///
    /// `buffer_size` sets the desired buffer_size. If None is passed in, then the
    /// default buffersize for the device is used.
    ///
    /// Both input and output streams need to be created together as a single slice of
    /// `ASIOBufferInfo`.
    fn create_streams(
        &self,
        mut input_buffer_infos: Vec<AsioBufferInfo>,
        mut output_buffer_infos: Vec<AsioBufferInfo>,
        buffer_size: Option<i32>,
    ) -> Result<AsioStreams, AsioError> {
        let (input, output) = match (
            input_buffer_infos.is_empty(),
            output_buffer_infos.is_empty(),
        ) {
            // Both stream exist.
            (false, false) => {
                // Create one continuous slice of buffers.
                let split_point = input_buffer_infos.len();
                let mut all_buffer_infos = input_buffer_infos;
                all_buffer_infos.append(&mut output_buffer_infos);
                // Create the buffers. On success, split the output and input again.
                let buffer_size = self.create_buffers(&mut all_buffer_infos, buffer_size)?;
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
            }
            // Just input
            (false, true) => {
                let buffer_size = self.create_buffers(&mut input_buffer_infos, buffer_size)?;
                let input = Some(AsioStream {
                    buffer_infos: input_buffer_infos,
                    buffer_size,
                });
                let output = None;
                (input, output)
            }
            // Just output
            (true, false) => {
                let buffer_size = self.create_buffers(&mut output_buffer_infos, buffer_size)?;
                let input = None;
                let output = Some(AsioStream {
                    buffer_infos: output_buffer_infos,
                    buffer_size,
                });
                (input, output)
            }
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
    /// `buffer_size` sets the desired buffer_size. If None is passed in, then the
    /// default buffersize for the device is used.
    ///
    /// This returns a full AsioStreams with both input and output if output was active.
    pub fn prepare_input_stream(
        &self,
        output: Option<AsioStream>,
        num_channels: usize,
        buffer_size: Option<i32>,
    ) -> Result<AsioStreams, AsioError> {
        let input_buffer_infos = prepare_buffer_infos(true, num_channels);
        let output_buffer_infos = output.map(|output| output.buffer_infos).unwrap_or_default();
        self.create_streams(input_buffer_infos, output_buffer_infos, buffer_size)
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
    /// `buffer_size` sets the desired buffer_size. If None is passed in, then the
    /// default buffersize for the device is used.
    ///
    /// This returns a full AsioStreams with both input and output if input was active.
    pub fn prepare_output_stream(
        &self,
        input: Option<AsioStream>,
        num_channels: usize,
        buffer_size: Option<i32>,
    ) -> Result<AsioStreams, AsioError> {
        let input_buffer_infos = input.map(|input| input.buffer_infos).unwrap_or_default();
        let output_buffer_infos = prepare_buffer_infos(false, num_channels);
        self.create_streams(input_buffer_infos, output_buffer_infos, buffer_size)
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
        let mut state = self.inner.lock_state();
        if let DriverState::Running = *state {
            return Ok(());
        }
        unsafe {
            asio_result!(ai::ASIOStart())?;
        }
        *state = DriverState::Running;
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
    pub fn add_callback<F>(&self, callback: F) -> BufferCallbackId
    where
        F: 'static + FnMut(&CallbackInfo) + Send,
    {
        let mut bc = BUFFER_CALLBACK.lock().unwrap();
        let id = bc
            .last()
            .map(|&(id, _)| BufferCallbackId(id.0.checked_add(1).expect("stream ID overflowed")))
            .unwrap_or(BufferCallbackId(0));
        let cb = BufferCallback(Box::new(callback));
        bc.push((id, cb));
        id
    }

    /// Remove the callback with the given ID.
    pub fn remove_callback(&self, rem_id: BufferCallbackId) {
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

    /// Register a callback to receive ASIO driver events.
    ///
    /// The callback receives an [`AsioDriverEvent`] and returns a `bool`. The return value is
    /// meaningful only for [`AsioDriverEvent::Message`] with selector
    /// [`AsioMessageSelectors::kAsioSelectorSupported`]: return `true` to advertise support for
    /// the queried selector, `false` to decline. For all other events the return value is ignored.
    ///
    /// Returns an ID uniquely associated with the given callback so that it may be removed later.
    pub fn add_event_callback<F>(&self, callback: F) -> DriverEventCallbackId
    where
        F: Fn(AsioDriverEvent) -> bool + Send + Sync + 'static,
    {
        let mut dcb = DRIVER_EVENT_CALLBACKS.lock().unwrap();
        let id = dcb
            .last()
            .map(|&(id, _)| {
                DriverEventCallbackId(
                    id.0.checked_add(1)
                        .expect("DriverEventCallbackId overflowed"),
                )
            })
            .unwrap_or(DriverEventCallbackId(0));

        let cb = DriverEventCallback(Arc::new(callback));
        dcb.push((id, cb));
        id
    }

    /// Remove the event callback with the given ID.
    pub fn remove_event_callback(&self, rem_id: DriverEventCallbackId) {
        let mut dcb = DRIVER_EVENT_CALLBACKS.lock().unwrap();
        dcb.retain(|&(id, _)| id != rem_id);
    }
}

impl DriverState {
    fn stop(&mut self) -> Result<(), AsioError> {
        if let DriverState::Running = *self {
            unsafe {
                asio_result!(ai::ASIOStop())?;
            }
            *self = DriverState::Prepared;
        }
        Ok(())
    }

    fn dispose_buffers(&mut self) -> Result<(), AsioError> {
        if let DriverState::Initialized = *self {
            return Ok(());
        }
        if let DriverState::Running = *self {
            self.stop()?;
        }
        unsafe {
            asio_result!(ai::ASIODisposeBuffers())?;
        }
        *self = DriverState::Initialized;
        Ok(())
    }

    fn destroy(&mut self) -> Result<(), AsioError> {
        if let DriverState::Running = *self {
            self.stop()?;
        }
        if let DriverState::Prepared = *self {
            self.dispose_buffers()?;
        }
        unsafe {
            asio_result!(ai::ASIOExit())?;
            ai::remove_current_driver();
        }
        Ok(())
    }
}

impl DriverInner {
    fn lock_state(&self) -> MutexGuard<'_, DriverState> {
        self.state.lock().expect("failed to lock `DriverState`")
    }

    fn stop_inner(&self) -> Result<(), AsioError> {
        let mut state = self.lock_state();
        state.stop()
    }

    fn dispose_buffers_inner(&self) -> Result<(), AsioError> {
        let mut state = self.lock_state();
        state.dispose_buffers()
    }

    fn destroy_inner(&mut self) -> Result<(), AsioError> {
        {
            let mut state = self.lock_state();
            state.destroy()?;

            // Clear any existing stream callbacks.
            if let Ok(mut bcs) = BUFFER_CALLBACK.lock() {
                bcs.clear();
            }
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
        .map(|ch| AsioBufferInfo {
            is_input,
            channel_num: ch as i32,
            // To be filled by ASIOCreateBuffers.
            buffers: [std::ptr::null_mut(); 2],
        })
        .collect()
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
    Ok(FromPrimitive::from_i32(channel_info.type_).expect("unknown `ASIOSampletype` value"))
}

/// ASIO uses null terminated c strings for driver names.
///
/// This converts to utf8.
fn driver_name_to_utf8(bytes: &[c_char]) -> std::borrow::Cow<'_, str> {
    unsafe { CStr::from_ptr(bytes.as_ptr()).to_string_lossy() }
}

/// Convert an `ASIOTimeStamp` (high and low 32-bit halves) to a `u64` nanosecond value.
#[inline]
fn asio_timestamp_to_nanos(ts: ai::ASIOTimeStamp) -> u64 {
    (ts.hi as u64) << 32 | ts.lo as u64
}

/// Indicates the stream sample rate has changed.
extern "C" fn sample_rate_did_change(s_rate: c_double) {
    let old_bits = CURRENT_SAMPLE_RATE.load(Ordering::Acquire);
    if s_rate.to_bits() != old_bits {
        CURRENT_SAMPLE_RATE.store(s_rate.to_bits(), Ordering::Release);
        dispatch_event(AsioDriverEvent::SampleRateChanged(s_rate));
    }
}

const ASIO_VERSION: c_long = 2;

/// Dispatch `event` to all registered driver event callbacks.
///
/// Returns `true` if any callback returns `true`. All callbacks are always called so that
/// notification side-effects (e.g. stream invalidation) reach every registered listener.
fn dispatch_event(event: AsioDriverEvent) -> bool {
    let callbacks: Vec<_> = {
        let lock = DRIVER_EVENT_CALLBACKS.lock().unwrap();
        lock.iter().map(|(_, cb)| cb.0.clone()).collect()
    };
    callbacks
        .iter()
        .fold(false, |handled, cb| cb(event) || handled)
}

/// Message callback for ASIO to notify of certain events.
extern "C" fn asio_message(
    selector: c_long,
    value: c_long,
    _message: *mut (),
    _opt: *mut c_double,
) -> c_long {
    match AsioMessageSelectors::from_i64(selector as i64) {
        Some(AsioMessageSelectors::kAsioSelectorSupported) => {
            // For selectors that asio-sys itself always handles, advertise support
            // unconditionally. For all others, delegate to registered callbacks so
            // each host can opt-in.
            match AsioMessageSelectors::from_i64(value as i64) {
                Some(AsioMessageSelectors::kAsioSelectorSupported)
                | Some(AsioMessageSelectors::kAsioResetRequest)
                | Some(AsioMessageSelectors::kAsioEngineVersion)
                | Some(AsioMessageSelectors::kAsioResyncRequest)
                | Some(AsioMessageSelectors::kAsioLatenciesChanged)
                | Some(AsioMessageSelectors::kAsioSupportsTimeInfo) => true as c_long,
                _ => dispatch_event(AsioDriverEvent::Message {
                    selector: AsioMessageSelectors::kAsioSelectorSupported,
                    value,
                }) as c_long,
            }
        }

        Some(AsioMessageSelectors::kAsioResetRequest) => {
            // The driver requests a full teardown and reinitialisation. Cannot be performed
            // here as this callback is invoked from within the driver; notify the host to
            // defer the reset to a safe point.
            dispatch_event(AsioDriverEvent::Message {
                selector: AsioMessageSelectors::kAsioResetRequest,
                value,
            });
            true as c_long
        }

        Some(AsioMessageSelectors::kAsioResyncRequest) => {
            // The driver encountered non-fatal data loss (e.g. a timestamp discontinuity).
            // Notify the host so it can handle the gap appropriately.
            dispatch_event(AsioDriverEvent::Message {
                selector: AsioMessageSelectors::kAsioResyncRequest,
                value,
            });
            true as c_long
        }

        Some(AsioMessageSelectors::kAsioLatenciesChanged) => {
            // The driver latencies have changed; have them re-queried.
            dispatch_event(AsioDriverEvent::Message {
                selector: AsioMessageSelectors::kAsioLatenciesChanged,
                value,
            });
            true as c_long
        }

        Some(AsioMessageSelectors::kAsioEngineVersion) => {
            // Return the supported ASIO version of the host application. If a host application
            // does not implement this selector, ASIO 1.0 is assumed by the driver.
            ASIO_VERSION
        }

        Some(AsioMessageSelectors::kAsioSupportsTimeInfo) => {
            // Informs the driver whether the asioCallbacks.bufferSwitchTimeInfo() callback is
            // supported. For compatibility with ASIO 1.0 drivers the host application should
            // always support the "old" bufferSwitch method, too, which we do.
            true as c_long
        }

        // For all other selectors, delegate to registered callbacks.
        Some(other) => dispatch_event(AsioDriverEvent::Message {
            selector: other,
            value,
        }) as c_long,

        None => false as c_long, // Unrecognised selector.
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
    let asio_time: &mut AsioTime = unsafe { &mut *(time as *mut AsioTime) };
    // Alternates: 0, 1, 0, 1, ...
    let callback_flag = CALLBACK_FLAG.fetch_xor(1, Ordering::Relaxed);

    let callback_info = CallbackInfo {
        buffer_index: double_buffer_index,
        system_time: asio_timestamp_to_nanos(asio_time.time_info.system_time),
        callback_flag,
    };
    for &mut (_, ref mut bc) in bcs.iter_mut() {
        bc.run(&callback_info);
    }

    if CALL_OUTPUT_READY.load(Ordering::Acquire) {
        unsafe { ai::ASIOOutputReady() };
    }

    time
}

/// This is called by ASIO.
///
/// Here we run the callback for each stream.
///
/// `double_buffer_index` is either `0` or `1`  indicating which buffer to fill.
extern "C" fn buffer_switch(double_buffer_index: c_long, direct_process: c_long) {
    // Emulate the time info provided by the `buffer_switch_time_info` callback.
    // This is an attempt at matching the behaviour in `hostsample.cpp` from the SDK.
    let mut time = unsafe {
        let mut time: AsioTime = std::mem::zeroed();
        let res = ai::ASIOGetSamplePosition(
            &mut time.time_info.sample_position,
            &mut time.time_info.system_time,
        );
        if let Ok(()) = asio_result!(res) {
            time.time_info.flags = (ai::AsioTimeInfoFlags::kSystemTimeValid
                | ai::AsioTimeInfoFlags::kSamplePositionValid)
                // Context about the cast:
                //
                // Cast was required to successfully compile with MinGW-w64.
                //
                // The flags defined will not create a value that exceeds the maximum value of an i32.
                // The flags are intended to be non-negative, so the sign bit will not be used.
                // The c_uint (flags) is being cast to i32 which is safe as long as the actual value fits within the i32 range, which is true in this case.
                //
                // The actual flags in asio sdk are defined as:
                // typedef enum AsioTimeInfoFlags
                // {
                //	kSystemTimeValid        = 1,            // must always be valid
                //	kSamplePositionValid    = 1 << 1,       // must always be valid
                //	kSampleRateValid        = 1 << 2,
                //	kSpeedValid             = 1 << 3,
                //
                //	kSampleRateChanged      = 1 << 4,
                //	kClockSourceChanged     = 1 << 5
                // } AsioTimeInfoFlags;
                .0 as _;
        }
        time
    };

    // Actual processing happens within the `buffer_switch_time_info` callback.
    let asio_time_ptr = &mut time as *mut AsioTime as *mut ai::ASIOTime;
    buffer_switch_time_info(asio_time_ptr, double_buffer_index, direct_process);
}

#[test]
fn check_type_sizes() {
    assert_eq!(
        std::mem::size_of::<AsioSampleRate>(),
        std::mem::size_of::<ai::ASIOSampleRate>()
    );
    assert_eq!(
        std::mem::size_of::<AsioTimeCode>(),
        std::mem::size_of::<ai::ASIOTimeCode>()
    );
    assert_eq!(
        std::mem::size_of::<AsioTimeInfo>(),
        std::mem::size_of::<ai::AsioTimeInfo>(),
    );
    assert_eq!(
        std::mem::size_of::<AsioTime>(),
        std::mem::size_of::<ai::ASIOTime>()
    );
}
