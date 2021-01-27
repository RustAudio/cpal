extern crate libc;
extern crate sndio_sys;

mod adapters;
mod runner;
use self::adapters::{input_adapter_callback, output_adapter_callback};
use self::runner::runner;

use std::collections::hash_map;
use std::collections::HashMap;
use std::convert::From;
use std::mem::{self, MaybeUninit};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use thiserror::Error;

use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, FrameCount, HostUnavailable, InputCallbackInfo,
    OutputCallbackInfo, PauseStreamError, PlayStreamError, SampleFormat, SampleRate, StreamConfig,
    StreamError, SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

use traits::{DeviceTrait, HostTrait, StreamTrait};

pub type SupportedInputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;

/// Default multiple of the round field of a sio_par struct to use for the buffer size (in frames).
const DEFAULT_ROUND_MULTIPLE: u32 = 2;

const DEFAULT_SAMPLE_RATE: SampleRate = SampleRate(48000);
const SUPPORTED_SAMPLE_RATES: &[SampleRate] =
    &[SampleRate(8000), SampleRate(44100), SampleRate(48000)];

#[derive(Clone, Debug, Error)]
pub enum SndioError {
    #[error("The requested device is no longer available. For example, it has been unplugged.")]
    DeviceNotAvailable,

    #[error("{0}")]
    BackendSpecific(BackendSpecificError),
}

#[cfg(target_endian = "big")]
const IS_LITTLE_ENDIAN: u32 = 0;

#[cfg(target_endian = "little")]
const IS_LITTLE_ENDIAN: u32 = 1;

impl From<SndioError> for BuildStreamError {
    fn from(e: SndioError) -> BuildStreamError {
        match e {
            SndioError::DeviceNotAvailable => BuildStreamError::DeviceNotAvailable,
            SndioError::BackendSpecific(bse) => BuildStreamError::BackendSpecific { err: bse },
        }
    }
}

impl From<SndioError> for DefaultStreamConfigError {
    fn from(e: SndioError) -> DefaultStreamConfigError {
        match e {
            SndioError::DeviceNotAvailable => DefaultStreamConfigError::DeviceNotAvailable,
            SndioError::BackendSpecific(bse) => {
                DefaultStreamConfigError::BackendSpecific { err: bse }
            }
        }
    }
}

impl From<SndioError> for PauseStreamError {
    fn from(e: SndioError) -> PauseStreamError {
        match e {
            SndioError::DeviceNotAvailable => PauseStreamError::DeviceNotAvailable,
            SndioError::BackendSpecific(bse) => PauseStreamError::BackendSpecific { err: bse },
        }
    }
}

impl From<SndioError> for StreamError {
    fn from(e: SndioError) -> StreamError {
        match e {
            SndioError::DeviceNotAvailable => StreamError::DeviceNotAvailable,
            SndioError::BackendSpecific(bse) => StreamError::BackendSpecific { err: bse },
        }
    }
}

impl From<SndioError> for SupportedStreamConfigsError {
    fn from(e: SndioError) -> SupportedStreamConfigsError {
        match e {
            SndioError::DeviceNotAvailable => SupportedStreamConfigsError::DeviceNotAvailable,
            SndioError::BackendSpecific(bse) => {
                SupportedStreamConfigsError::BackendSpecific { err: bse }
            }
        }
    }
}

pub struct Devices(Option<Device>);

impl Devices {
    fn new() -> Self {
        Devices(Some(Device::new()))
    }
}

impl Iterator for Devices {
    type Item = Device;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.take()
    }
}

struct SioHdl(*mut sndio_sys::sio_hdl);

impl SioHdl {
    /// Returns a map of sample rates to sio_par by re-configuring the device. This should not be
    /// performed while recording or playing, or even after configuring the device for this state!
    fn get_sample_rate_map(
        &mut self,
        behavior: BufferXrunBehavior,
    ) -> Result<SampleRateMap, SndioError> {
        let mut sample_rate_map = SampleRateMap::new();
        for rate in SUPPORTED_SAMPLE_RATES {
            let mut par = new_sio_par();

            // Use I16 at 48KHz; mono playback & record
            par.bits = 16;
            par.sig = 1;
            par.le = IS_LITTLE_ENDIAN; // Native byte order
            par.rchan = 1; // mono record
            par.pchan = 1; // mono playback
            par.rate = rate.0;
            par.xrun = match behavior {
                BufferXrunBehavior::Ignore => 0,
                BufferXrunBehavior::Sync => 1,
                BufferXrunBehavior::Error => 2,
            };

            // Set it on device and get it back to see what is valid.
            self.negotiate_params(&mut par)?;

            if par.rchan != 1 {
                return Err(backend_specific_error(format!(
                    "unexpected number of record channels: {}",
                    par.rchan
                )));
            }

            if par.pchan != 1 {
                return Err(backend_specific_error(format!(
                    "unexpected number of playback channels: {}",
                    par.pchan
                )));
            }

            if par.rate != rate.0 {
                return Err(backend_specific_error(format!(
                    "unexpected sample rate (frames per second): expected {}, got {}",
                    rate.0, par.rate
                )));
            }

            // TODO: more checks -- bits, bps, sig, le, msb

            sample_rate_map.insert(*rate, par);
        }
        Ok(sample_rate_map)
    }

    /// Calls sio_setpar and sio_getpar on the passed in sio_par struct. Before calling this, the
    /// caller should have initialized `par` with `new_sio_par` and then set the desired parameters
    /// on it. After calling (assuming an error is not returned), the caller should check the
    /// parameters to see if they are OK.
    ///
    /// This should not be called if the device is running! However, it will panic if the device is
    /// not opened yet.
    fn negotiate_params(&mut self, par: &mut sndio_sys::sio_par) -> Result<(), SndioError> {
        // What follows is the suggested parameter negotiation from the man pages.
        self.set_params(par)?;

        let status = unsafe {
            // Retrieve the actual parameters of the device.
            sndio_sys::sio_getpar(self.0, par as *mut _)
        };
        if status != 1 {
            return Err(backend_specific_error(
                "failed to get device-supported parameters with sio_getpar",
            )
            .into());
        }

        if par.bits != 16 || par.bps != 2 {
            // We have to check both because of the possibility of padding (usually an issue with
            // 24 bits not 16 though).
            return Err(backend_specific_error(format!(
                "unexpected sample size (not 16bit): bits/sample: {}, bytes/sample: {})",
                par.bits, par.bps
            ))
            .into());
        }

        if par.sig != 1 {
            return Err(backend_specific_error(
                "sndio device does not support I16 but we need it to",
            )
            .into());
        }
        Ok(())
    }

    /// Calls sio_setpar on the passed in sio_par struct. This sets the device parameters.
    fn set_params(&mut self, par: &sndio_sys::sio_par) -> Result<(), SndioError> {
        let mut newpar = new_sio_par();
        // This is a little hacky -- testing indicates the __magic from sio_initpar needs to be
        // preserved when calling sio_setpar. Unfortunately __magic is the wrong value after
        // retrieval from sio_getpar.
        newpar.bits = par.bits;
        newpar.bps = par.bps;
        newpar.sig = par.sig;
        newpar.le = par.le;
        newpar.msb = par.msb;
        newpar.rchan = par.rchan;
        newpar.pchan = par.pchan;
        newpar.rate = par.rate;
        newpar.appbufsz = par.appbufsz;
        newpar.bufsz = par.bufsz;
        newpar.round = par.round;
        newpar.xrun = par.xrun;
        let status = unsafe {
            // Request the device using our parameters
            sndio_sys::sio_setpar(self.0, &mut newpar as *mut _)
        };
        if status != 1 {
            return Err(backend_specific_error("failed to set parameters with sio_setpar").into());
        }
        Ok(())
    }
}

// It is necessary to add the Send marker trait to this struct because the Arc<Mutex<InnerState>>
// which contains it needs to be passed to the runner thread. This is sound as long as the sio_hdl
// pointer is not copied out of its SioHdl and used while the mutex is unlocked.
unsafe impl Send for SioHdl {}

struct SampleRateMap(pub HashMap<u32, sndio_sys::sio_par>);

impl SampleRateMap {
    fn new() -> Self {
        SampleRateMap(HashMap::new())
    }

    fn get(&self, rate: SampleRate) -> Option<&sndio_sys::sio_par> {
        self.0.get(&rate.0)
    }

    fn insert(&mut self, rate: SampleRate, par: sndio_sys::sio_par) -> Option<sndio_sys::sio_par> {
        self.0.insert(rate.0, par)
    }

    fn iter(&self) -> SampleRateMapIter<'_> {
        SampleRateMapIter {
            iter: self.0.iter(),
        }
    }
}

struct SampleRateMapIter<'a> {
    iter: hash_map::Iter<'a, u32, sndio_sys::sio_par>,
}

impl<'a> Iterator for SampleRateMapIter<'a> {
    type Item = (SampleRate, &'a sndio_sys::sio_par);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|(sample_rate, par)| (SampleRate(*sample_rate), par))
    }
}

/// The shared state between `Device` and `Stream`. Responsible for closing handle when dropped.
/// Upon `Device` creation, this is in the `Init` state. Calling `.open` transitions
/// this to the `Opened` state (this generally happens when getting input or output configs).
/// From there, the state can transition to `Running` once at least one `Stream` has been created
/// with the `build_input_stream_raw` or `build_output_stream_raw` functions.
enum InnerState {
    Init {
        /// Buffer overrun/underrun behavior -- ignore/sync/error?
        behavior: BufferXrunBehavior,
    },
    Opened {
        /// Contains a handle returned from sio_open. Note that even though this is a pointer type
        /// and so doesn't follow Rust's borrowing rules, we should be careful not to copy it out
        /// because that may render Mutex<InnerState> ineffective in enforcing exclusive access.
        hdl: SioHdl,

        /// Map of sample rate to parameters.
        sample_rate_map: SampleRateMap,
    },
    Running {
        /// Contains a handle returned from sio_open. Note that even though this is a pointer type
        /// and so doesn't follow Rust's borrowing rules, we should be careful not to copy it out
        /// because that may render Mutex<InnerState> ineffective in enforcing exclusive access.
        hdl: SioHdl,

        /// Contains the chosen buffer size, in elements not bytes.
        buffer_size: FrameCount,

        /// Stores the sndio-configured parameters.
        par: sndio_sys::sio_par,

        /// Map of sample rate to parameters.
        sample_rate_map: SampleRateMap,

        /// Each input Stream that has not been dropped has its callbacks in an element of this Vec.
        /// The last element is guaranteed to not be None.
        input_callbacks: (usize, HashMap<usize, InputCallbacks>),

        /// Each output Stream that has not been dropped has its callbacks in an element of this Vec.
        /// The last element is guaranteed to not be None.
        output_callbacks: (usize, HashMap<usize, OutputCallbacks>),

        /// Whether the runner thread was spawned yet.
        thread_spawned: bool,

        /// Channel used for signalling that the runner thread should wakeup because there is now a
        /// Stream. This will only be None if either 1) the runner thread has not yet started and
        /// set this value, or 2) the runner thread has exited.
        wakeup_sender: Option<mpsc::Sender<()>>,
    },
}

struct InputCallbacks {
    data_callback: Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>,
    error_callback: Box<dyn FnMut(StreamError) + Send + 'static>,
}

struct OutputCallbacks {
    data_callback: Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>,
    error_callback: Box<dyn FnMut(StreamError) + Send + 'static>,
}

impl InnerState {
    fn new() -> Self {
        InnerState::Init {
            behavior: BufferXrunBehavior::Sync,
        }
    }

    fn open(&mut self) -> Result<(), SndioError> {
        match self {
            InnerState::Opened { .. } | InnerState::Running { .. } => {
                Err(backend_specific_error("device is already open"))
            }
            InnerState::Init { ref behavior } => {
                let hdl = unsafe {
                    // The transmute is needed because this C string is *const u8 in one place but *const i8 in another place.
                    let devany_ptr =
                        mem::transmute::<_, *const i8>(sndio_sys::SIO_DEVANY as *const _);
                    let nonblocking = true as i32;
                    sndio_sys::sio_open(
                        devany_ptr,
                        sndio_sys::SIO_PLAY | sndio_sys::SIO_REC,
                        nonblocking,
                    )
                };
                if hdl.is_null() {
                    return Err(SndioError::DeviceNotAvailable);
                }

                let mut hdl = SioHdl(hdl);
                let sample_rate_map = hdl.get_sample_rate_map(*behavior)?;
                *self = InnerState::Opened {
                    hdl,
                    sample_rate_map,
                };
                Ok(())
            }
        }
    }

    fn start(&mut self) -> Result<(), SndioError> {
        match self {
            InnerState::Running { hdl, .. } => {
                let status = unsafe {
                    // "The sio_start() function puts the device in a waiting state: the device
                    // will wait for playback data to be provided (using the sio_write()
                    // function).  Once enough data is queued to ensure that play buffers will
                    // not underrun, actual playback is started automatically."
                    sndio_sys::sio_start(hdl.0) // Unwrap OK because of check above
                };
                if status != 1 {
                    Err(backend_specific_error("failed to start stream"))
                } else {
                    Ok(())
                }
            }
            _ => Err(backend_specific_error(
                "cannot start a device that hasn't been opened yet",
            )),
        }
    }

    fn stop(&mut self) -> Result<(), SndioError> {
        match self {
            InnerState::Running { hdl, .. } => {
                let status = unsafe {
                    // The sio_stop() function puts the audio subsystem in the same state as before
                    // sio_start() is called.  It stops recording, drains the play buffer and then stops
                    // playback.  If samples to play are queued but playback hasn't started yet then
                    // playback is forced immediately; playback will actually stop once the buffer is
                    // drained.  In no case are samples in the play buffer discarded.
                    sndio_sys::sio_stop(hdl.0) // Unwrap OK because of check above
                };
                if status != 1 {
                    Err(backend_specific_error("error calling sio_stop"))
                } else {
                    Ok(())
                }
            }
            _ => {
                // Nothing to do -- device is not open.
                Ok(())
            }
        }
    }

    // TODO: make these 4 methods generic (new CallbackSet<T> where T is either InputCallbacks or OutputCallbacks)
    /// Puts the supplied callbacks into the vector in the first free position, or at the end. The
    /// index of insertion is returned.
    fn add_output_callbacks(&mut self, callbacks: OutputCallbacks) -> Result<usize, SndioError> {
        match self {
            InnerState::Running {
                ref input_callbacks,
                ref mut output_callbacks,
                ref mut wakeup_sender,
                ..
            } => {
                // If there were previously no callbacks, wakeup the runner thread.
                if input_callbacks.1.len() == 0 && output_callbacks.1.len() == 0 {
                    if let Some(ref sender) = wakeup_sender {
                        let _ = sender.send(());
                    }
                }
                let index = output_callbacks.0;
                output_callbacks.1.insert(index, callbacks);
                output_callbacks.0 = index + 1;
                Ok(index)
            }
            _ => Err(backend_specific_error("device is not in a running state")),
        }
    }

    /// Removes the callbacks at specified index, returning them. Panics if the index is invalid
    /// (out of range or there is a None element at that position).
    fn remove_output_callbacks(&mut self, index: usize) -> Result<OutputCallbacks, SndioError> {
        match *self {
            InnerState::Running {
                ref mut output_callbacks,
                ..
            } => Ok(output_callbacks.1.remove(&index).unwrap()),
            _ => Err(backend_specific_error("device is not in a running state")),
        }
    }

    /// Puts the supplied callbacks into the vector in the first free position, or at the end. The
    /// index of insertion is returned.
    fn add_input_callbacks(&mut self, callbacks: InputCallbacks) -> Result<usize, SndioError> {
        match self {
            InnerState::Running {
                ref mut input_callbacks,
                ref output_callbacks,
                ref mut wakeup_sender,
                ..
            } => {
                // If there were previously no callbacks, wakeup the runner thread.
                if input_callbacks.1.len() == 0 && output_callbacks.1.len() == 0 {
                    if let Some(ref sender) = wakeup_sender {
                        let _ = sender.send(());
                    }
                }
                let index = input_callbacks.0;
                input_callbacks.1.insert(index, callbacks);
                input_callbacks.0 = index + 1;
                Ok(index)
            }
            _ => Err(backend_specific_error("device is not in a running state")),
        }
    }

    /// Removes the callbacks at specified index, returning them. Panics if the index is invalid
    /// (out of range or there is a None element at that position).
    fn remove_input_callbacks(&mut self, index: usize) -> Result<InputCallbacks, SndioError> {
        match *self {
            InnerState::Running {
                ref mut input_callbacks,
                ..
            } => Ok(input_callbacks.1.remove(&index).unwrap()),
            _ => Err(backend_specific_error("device is not in a running state")),
        }
    }

    /// Send an error to all input and output error callbacks.
    fn error(&mut self, e: impl Into<StreamError>) {
        match *self {
            InnerState::Running {
                ref mut input_callbacks,
                ref mut output_callbacks,
                ..
            } => {
                let e = e.into();
                for cbs in input_callbacks.1.values_mut() {
                    (cbs.error_callback)(e.clone());
                }
                for cbs in output_callbacks.1.values_mut() {
                    (cbs.error_callback)(e.clone());
                }
            }
            _ => {} // Drop the error
        }
    }

    /// Common code shared between build_input_stream_raw and build_output_stream_raw
    fn setup_stream(&mut self, config: &StreamConfig) -> Result<(), BuildStreamError> {
        // If not already open, make sure it's open
        match self {
            InnerState::Init { .. } => {
                self.open()?;
            }
            _ => {}
        }

        match self {
            InnerState::Init { .. } => {
                // Probably unreachable
                Err(backend_specific_error("device was expected to be opened").into())
            }
            InnerState::Opened {
                hdl,
                sample_rate_map,
            } => {
                // No running streams yet; we get to set the par.
                let mut par;
                if let Some(par_) = sample_rate_map.get(config.sample_rate) {
                    par = par_.clone();
                } else {
                    return Err(backend_specific_error(format!(
                        "no configuration for sample rate {}",
                        config.sample_rate.0
                    ))
                    .into());
                }

                let buffer_size = determine_buffer_size(&config.buffer_size, par.round, None)?;

                // Transition to running
                par.appbufsz = buffer_size as u32;
                hdl.set_params(&par)?;
                let mut tmp = SampleRateMap::new();
                mem::swap(&mut tmp, sample_rate_map);

                *self = InnerState::Running {
                    hdl: SioHdl(hdl.0), // Just this once, it's ok to copy this
                    buffer_size,
                    par,
                    sample_rate_map: tmp,
                    input_callbacks: (0, HashMap::new()),
                    output_callbacks: (0, HashMap::new()),
                    thread_spawned: false,
                    wakeup_sender: None,
                };
                Ok(())
            }
            InnerState::Running {
                par, buffer_size, ..
            } => {
                // TODO: allow setting new par like above flow if input_callbacks and
                // output_callbacks are both zero.

                // Perform some checks
                if par.rate != config.sample_rate.0 as u32 {
                    return Err(backend_specific_error("sample rates don't match").into());
                }
                determine_buffer_size(&config.buffer_size, par.round, Some(*buffer_size))?;
                Ok(())
            }
        }
    }

    fn has_streams(&self) -> bool {
        match self {
            InnerState::Running {
                ref input_callbacks,
                ref output_callbacks,
                ..
            } => input_callbacks.1.len() > 0 || output_callbacks.1.len() > 0,
            _ => false,
        }
    }
}

impl Drop for InnerState {
    fn drop(&mut self) {
        match self {
            InnerState::Running { hdl, .. } => unsafe {
                sndio_sys::sio_close(hdl.0);
            },
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum BufferXrunBehavior {
    Ignore, // SIO_IGNORE
    Sync,   // SIO_SYNC
    Error,  // SIO_ERROR
}

#[derive(Clone)]
pub struct Device {
    inner_state: Arc<Mutex<InnerState>>,
}

impl Device {
    pub fn new() -> Self {
        Device {
            inner_state: Arc::new(Mutex::new(InnerState::new())),
        }
    }

    pub fn set_xrun_behavior(&mut self, b: BufferXrunBehavior) -> Result<(), SndioError> {
        let mut inner_state = self.inner_state.lock().map_err(|e| {
            backend_specific_error(format!("InnerState unlock error: {:?}", e)).into()
        })?;
        match *inner_state {
            InnerState::Init {
                ref mut behavior, ..
            } => {
                *behavior = b;
                Ok(())
            }
            _ => Err(backend_specific_error(
                "the xrun behavior can only be specified at initialization time",
            )),
        }
    }
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("sndio default device".to_owned())
    }

    #[inline]
    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        let mut inner_state =
            self.inner_state
                .lock()
                .map_err(|e| -> SupportedStreamConfigsError {
                    backend_specific_error(format!("InnerState unlock error: {:?}", e)).into()
                })?;

        match *inner_state {
            InnerState::Init { .. } => {
                inner_state.open()?;
            }
            _ => {}
        }

        match *inner_state {
            InnerState::Running {
                ref sample_rate_map,
                ..
            }
            | InnerState::Opened {
                ref sample_rate_map,
                ..
            } => {
                let mut config_ranges = vec![];
                for (_, par) in sample_rate_map.iter() {
                    let config = supported_config_from_par(par, par.rchan);
                    config_ranges.push(SupportedStreamConfigRange {
                        channels: config.channels,
                        min_sample_rate: config.sample_rate,
                        max_sample_rate: config.sample_rate,
                        buffer_size: config.buffer_size,
                        sample_format: config.sample_format,
                    });
                }

                Ok(config_ranges.into_iter())
            }
            _ => Err(backend_specific_error("device has not yet been opened").into()),
        }
    }

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        let mut inner_state =
            self.inner_state
                .lock()
                .map_err(|e| -> SupportedStreamConfigsError {
                    backend_specific_error(format!("InnerState unlock error: {:?}", e)).into()
                })?;

        match *inner_state {
            InnerState::Init { .. } => {
                inner_state.open()?;
            }
            _ => {}
        }

        match *inner_state {
            InnerState::Running {
                ref sample_rate_map,
                ..
            }
            | InnerState::Opened {
                ref sample_rate_map,
                ..
            } => {
                let mut config_ranges = vec![];
                for (_, par) in sample_rate_map.iter() {
                    let config = supported_config_from_par(par, par.pchan);
                    config_ranges.push(SupportedStreamConfigRange {
                        channels: config.channels,
                        min_sample_rate: config.sample_rate,
                        max_sample_rate: config.sample_rate,
                        buffer_size: config.buffer_size,
                        sample_format: config.sample_format,
                    });
                }

                Ok(config_ranges.into_iter())
            }
            _ => Err(backend_specific_error("device has not yet been opened").into()),
        }
    }

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut inner_state = self
            .inner_state
            .lock()
            .map_err(|e| -> DefaultStreamConfigError {
                backend_specific_error(format!("InnerState unlock error: {:?}", e)).into()
            })?;

        match *inner_state {
            InnerState::Init { .. } => {
                inner_state.open()?;
            }
            _ => {}
        }

        match *inner_state {
            InnerState::Running {
                ref sample_rate_map,
                ..
            }
            | InnerState::Opened {
                ref sample_rate_map,
                ..
            } => {
                let config = if let Some(par) = sample_rate_map.get(DEFAULT_SAMPLE_RATE) {
                    supported_config_from_par(par, par.rchan)
                } else {
                    return Err(backend_specific_error(
                        "missing map of sample rates to sio_par structs!",
                    )
                    .into());
                };

                Ok(config)
            }
            _ => Err(backend_specific_error("device has not yet been opened").into()),
        }
    }

    #[inline]
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut inner_state = self
            .inner_state
            .lock()
            .map_err(|e| -> DefaultStreamConfigError {
                backend_specific_error(format!("InnerState unlock error: {:?}", e)).into()
            })?;

        match *inner_state {
            InnerState::Init { .. } => {
                inner_state.open()?;
            }
            _ => {}
        }

        match *inner_state {
            InnerState::Running {
                ref sample_rate_map,
                ..
            }
            | InnerState::Opened {
                ref sample_rate_map,
                ..
            } => {
                let config = if let Some(par) = sample_rate_map.get(DEFAULT_SAMPLE_RATE) {
                    supported_config_from_par(par, par.pchan)
                } else {
                    return Err(backend_specific_error(
                        "missing map of sample rates to sio_par structs!",
                    )
                    .into());
                };

                Ok(config)
            }
            _ => Err(backend_specific_error("device has not yet been opened").into()),
        }
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let inner_state_arc = self.inner_state.clone();

        let mut inner_state = self.inner_state.lock().unwrap();

        inner_state.setup_stream(config)?;

        let idx;
        let boxed_data_cb;
        match *inner_state {
            InnerState::Init { .. } | InnerState::Opened { .. } => {
                return Err(backend_specific_error("stream was not properly setup").into());
            }
            InnerState::Running {
                ref buffer_size, ..
            } => {
                boxed_data_cb = if sample_format != SampleFormat::I16 {
                    input_adapter_callback(data_callback, *buffer_size, sample_format)
                } else {
                    Box::new(data_callback)
                };
            }
        }

        idx = inner_state.add_input_callbacks(InputCallbacks {
            data_callback: boxed_data_cb,
            error_callback: Box::new(error_callback),
        })?;

        match *inner_state {
            InnerState::Init { .. } | InnerState::Opened { .. } => {}
            InnerState::Running {
                ref mut thread_spawned,
                ..
            } => {
                if !*thread_spawned {
                    thread::spawn(move || runner(inner_state_arc));
                    *thread_spawned = true;
                }
            }
        }

        drop(inner_state); // Unlock
        Ok(Stream {
            inner_state: self.inner_state.clone(),
            is_output: false,
            index: idx,
        })
    }

    /// Create an output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let inner_state_arc = self.inner_state.clone();

        let mut inner_state = self.inner_state.lock().map_err(|e| -> BuildStreamError {
            backend_specific_error(format!("InnerState unlock error: {:?}", e)).into()
        })?;

        inner_state.setup_stream(config)?;

        let idx;
        let boxed_data_cb;
        match *inner_state {
            InnerState::Init { .. } | InnerState::Opened { .. } => {
                return Err(backend_specific_error("stream was not properly setup").into());
            }
            InnerState::Running {
                ref buffer_size, ..
            } => {
                boxed_data_cb = if sample_format != SampleFormat::I16 {
                    output_adapter_callback(data_callback, *buffer_size, sample_format)
                } else {
                    Box::new(data_callback)
                };
            }
        }

        idx = inner_state.add_output_callbacks(OutputCallbacks {
            data_callback: boxed_data_cb,
            error_callback: Box::new(error_callback),
        })?;

        match *inner_state {
            InnerState::Init { .. } | InnerState::Opened { .. } => {}
            InnerState::Running {
                ref mut thread_spawned,
                ..
            } => {
                if !*thread_spawned {
                    thread::spawn(move || runner(inner_state_arc));
                    *thread_spawned = true;
                }
            }
        }

        drop(inner_state); // Unlock
        Ok(Stream {
            inner_state: self.inner_state.clone(),
            is_output: true,
            index: idx,
        })
    }
}

fn supported_config_from_par(par: &sndio_sys::sio_par, num_channels: u32) -> SupportedStreamConfig {
    SupportedStreamConfig {
        channels: num_channels as u16, // Conversion is courtesy of type mismatch between sndio and RustAudio.
        sample_rate: SampleRate(par.rate), // TODO: actually frames per second, not samples per second. Important for adding multi-channel support
        buffer_size: SupportedBufferSize::Range {
            min: par.round,
            max: 10 * par.round, // There isn't really a max.
                                 // Also note that min and max hold frame counts not
                                 // sample counts. This would matter if stereo was
                                 // supported.
        },
        sample_format: SampleFormat::I16,
    }
}

fn new_sio_par() -> sndio_sys::sio_par {
    let mut par = MaybeUninit::<sndio_sys::sio_par>::uninit();
    unsafe {
        sndio_sys::sio_initpar(par.as_mut_ptr());
        par.assume_init()
    }
}

fn backend_specific_error(desc: impl Into<String>) -> SndioError {
    SndioError::BackendSpecific(BackendSpecificError {
        description: desc.into(),
    })
}

pub struct Host;

impl Host {
    pub fn new() -> Result<Host, HostUnavailable> {
        Ok(Host)
    }

    pub fn default_output_device() -> Option<Device> {
        Some(Device::new())
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume this host is always available on sndio.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Ok(Devices::new())
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        Some(Device::new())
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        Some(Device::new())
    }
}

pub struct Stream {
    inner_state: Arc<Mutex<InnerState>>,

    /// True if this is output; false if this is input.
    is_output: bool,

    /// Index into input_callbacks or output_callbacks
    index: usize,
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        // No-op since the stream was already started by build_output_stream_raw
        Ok(())
    }

    // sndio doesn't support pausing.
    fn pause(&self) -> Result<(), PauseStreamError> {
        Err(backend_specific_error("pausing is not implemented").into())
    }
}

impl Drop for Stream {
    /// Requests a shutdown from the callback (runner) thread and waits for it to finish shutting down.
    /// If the thread is already stopped, nothing happens.
    fn drop(&mut self) {
        let mut inner_state = self.inner_state.lock().unwrap();
        if self.is_output {
            let _ = inner_state.remove_output_callbacks(self.index);
        } else {
            let _ = inner_state.remove_input_callbacks(self.index);
        }

        match *inner_state {
            InnerState::Running {
                ref thread_spawned,
                ref wakeup_sender,
                ..
            } => {
                if !inner_state.has_streams() && *thread_spawned {
                    // Wake up runner thread so it can shut down
                    if let Some(ref sender) = wakeup_sender {
                        let _ = sender.send(());
                    }
                }
            }
            _ => {}
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let inner_state = self.inner_state.lock().unwrap();
        match *inner_state {
            InnerState::Running {
                ref thread_spawned,
                ref wakeup_sender,
                ..
            } => {
                if !inner_state.has_streams() && *thread_spawned {
                    // Wake up runner thread so it can shut down
                    if let Some(ref sender) = wakeup_sender {
                        let _ = sender.send(());
                    }
                }
            }
            _ => {}
        }
    }
}

fn determine_buffer_size(
    requested: &BufferSize,
    round: FrameCount,
    configured_size: Option<FrameCount>,
) -> Result<FrameCount, SndioError> {
    // Round up the buffer size the user selected to the next multiple of par.round. If there
    // was already a stream created with a different buffer size, return an error (sorry).
    // Note: if we want stereo support, this will need to change.
    let desired_buffer_size = match requested {
        BufferSize::Fixed(requested) => {
            let requested = *requested;
            if requested > 0 {
                requested + round - ((requested - 1) % round) - 1
            } else {
                round
            }
        }
        BufferSize::Default => {
            if let Some(bufsize) = configured_size {
                bufsize
            } else {
                DEFAULT_ROUND_MULTIPLE * round
            }
        }
    };

    if configured_size.is_some() && configured_size != Some(desired_buffer_size) {
        return Err(backend_specific_error("buffer sizes don't match").into());
    }
    Ok(desired_buffer_size)
}
