extern crate libc;
extern crate sndio_sys;

mod adapters;
mod runner;
use self::adapters::{input_adapter_callback, output_adapter_callback};
use self::runner::runner;

use std::collections::HashMap;
use std::convert::From;
use std::mem::{self, MaybeUninit};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use thiserror::Error;

use crate::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceNameError, DevicesError, HostUnavailable, InputCallbackInfo, OutputCallbackInfo,
    PauseStreamError, PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedBufferSize, SupportedStreamConfig, SupportedStreamConfigRange,
    SupportedStreamConfigsError,
};

use traits::{DeviceTrait, HostTrait, StreamTrait};

pub type SupportedInputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = ::std::vec::IntoIter<SupportedStreamConfigRange>;

/// Default multiple of the round field of a sio_par struct to use for the buffer size.
const DEFAULT_ROUND_MULTIPLE: usize = 2;

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

pub struct Devices {
    returned: bool,
}

impl Iterator for Devices {
    type Item = Device;
    fn next(&mut self) -> Option<Device> {
        if self.returned {
            None
        } else {
            self.returned = true;
            Some(Device::new())
        }
    }
}

impl Devices {
    fn new() -> Devices {
        Devices { returned: false }
    }
}

/// The shared state between Device and Stream. Responsible for closing handle when dropped.
struct InnerState {
    /// If device has been open with sio_open, contains a handle. Note that even though this is a
    /// pointer type and so doesn't follow Rust's borrowing rules, we should be careful not to copy
    /// it out because that may render Mutex<InnerState> ineffective in enforcing exclusive access.
    hdl: Option<*mut sndio_sys::sio_hdl>,

    /// Buffer overrun/underrun behavior -- ignore/sync/error?
    behavior: BufferXrunBehavior,

    /// If a buffer size was chosen, contains that value.
    buffer_size: Option<usize>,

    /// If the device was configured, stores the sndio-configured parameters.
    par: Option<sndio_sys::sio_par>,

    /// Map of sample rate to parameters.
    /// Guaranteed to not be None if hdl is not None.
    sample_rate_to_par: Option<HashMap<u32, sndio_sys::sio_par>>,

    /// Indicates if the read/write thread is started, shutting down, or stopped.
    status: Status,

    /// Each input Stream that has not been dropped has its callbacks in an element of this Vec.
    /// The last element is guaranteed to not be None.
    input_callbacks: Vec<Option<InputCallbacks>>,

    /// Each output Stream that has not been dropped has its callbacks in an element of this Vec.
    /// The last element is guaranteed to not be None.
    output_callbacks: Vec<Option<OutputCallbacks>>,

    /// Channel used for signalling that the runner thread should wakeup because there is now a
    /// Stream. This will only be None if there is no runner thread.
    wakeup_sender: Option<mpsc::Sender<()>>,
}

struct InputCallbacks {
    data_callback: Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>,
    error_callback: Box<dyn FnMut(StreamError) + Send + 'static>,
}

struct OutputCallbacks {
    data_callback: Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>,
    error_callback: Box<dyn FnMut(StreamError) + Send + 'static>,
}

unsafe impl Send for InnerState {}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Status {
    /// Initial state. No thread running. Device/Stream methods will start thread and change this
    /// to Running.
    Stopped,

    /// Thread is running (unless it encountered an error).
    Running,
}

impl InnerState {
    fn new() -> Self {
        InnerState {
            hdl: None,
            behavior: BufferXrunBehavior::Sync,
            par: None,
            sample_rate_to_par: None,
            buffer_size: None,
            status: Status::Stopped,
            input_callbacks: vec![],
            output_callbacks: vec![],
            wakeup_sender: None,
        }
    }

    fn open(&mut self) -> Result<(), SndioError> {
        if self.hdl.is_some() {
            // Already open
            return Ok(());
        }

        let hdl = unsafe {
            // The transmute is needed because this C string is *const u8 in one place but *const i8 in another place.
            let devany_ptr = mem::transmute::<_, *const i8>(sndio_sys::SIO_DEVANY as *const _);
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
        self.hdl = Some(hdl);

        let mut sample_rate_to_par = HashMap::new();
        for rate in SUPPORTED_SAMPLE_RATES {
            let mut par = new_sio_par();

            // Use I16 at 48KHz; mono playback & record
            par.bits = 16;
            par.sig = 1;
            par.le = IS_LITTLE_ENDIAN; // Native byte order
            par.rchan = 1; // mono record
            par.pchan = 1; // mono playback
            par.rate = rate.0;
            par.xrun = match self.behavior {
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

            sample_rate_to_par.insert(rate.0, par);
        }
        self.sample_rate_to_par = Some(sample_rate_to_par);
        Ok(())
    }

    fn start(&mut self) -> Result<(), SndioError> {
        if self.hdl.is_none() {
            return Err(backend_specific_error(
                "cannot start a device that hasn't been opened yet",
            ));
        }
        let status = unsafe {
            // "The sio_start() function puts the device in a waiting state: the device
            // will wait for playback data to be provided (using the sio_write()
            // function).  Once enough data is queued to ensure that play buffers will
            // not underrun, actual playback is started automatically."
            sndio_sys::sio_start(self.hdl.unwrap()) // Unwrap OK because of check above
        };
        if status != 1 {
            return Err(backend_specific_error("failed to start stream"));
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), SndioError> {
        if self.hdl.is_none() {
            // Nothing to do -- device is not open.
            return Ok(());
        }
        let status = unsafe {
            // The sio_stop() function puts the audio subsystem in the same state as before
            // sio_start() is called.  It stops recording, drains the play buffer and then stops
            // playback.  If samples to play are queued but playback hasn't started yet then
            // playback is forced immediately; playback will actually stop once the buffer is
            // drained.  In no case are samples in the play buffer discarded.
            sndio_sys::sio_stop(self.hdl.unwrap()) // Unwrap OK because of check above
        };
        if status != 1 {
            return Err(backend_specific_error("error calling sio_stop"));
        }
        Ok(())
    }

    // TODO: make these 4 methods generic (new CallbackSet<T> where T is either InputCallbacks or OutputCallbacks)
    /// Puts the supplied callbacks into the vector in the first free position, or at the end. The
    /// index of insertion is returned.
    fn add_output_callbacks(&mut self, callbacks: OutputCallbacks) -> usize {
        for (i, cbs) in self.output_callbacks.iter_mut().enumerate() {
            if cbs.is_none() {
                *cbs = Some(callbacks);
                return i;
            }
        }
        // If there were previously no callbacks, wakeup the runner thread.
        if self.input_callbacks.len() == 0 && self.output_callbacks.len() == 0 {
            if let Some(ref sender) = self.wakeup_sender {
                let _ = sender.send(());
            }
        }
        self.output_callbacks.push(Some(callbacks));
        self.output_callbacks.len() - 1
    }

    /// Removes the callbacks at specified index, returning them. Panics if the index is invalid
    /// (out of range or there is a None element at that position).
    fn remove_output_callbacks(&mut self, index: usize) -> OutputCallbacks {
        let cbs = self.output_callbacks[index].take().unwrap();
        while self.output_callbacks.len() > 0
            && self.output_callbacks[self.output_callbacks.len() - 1].is_none()
        {
            self.output_callbacks.pop();
        }
        cbs
    }

    /// Puts the supplied callbacks into the vector in the first free position, or at the end. The
    /// index of insertion is returned.
    fn add_input_callbacks(&mut self, callbacks: InputCallbacks) -> usize {
        for (i, cbs) in self.input_callbacks.iter_mut().enumerate() {
            if cbs.is_none() {
                *cbs = Some(callbacks);
                return i;
            }
        }
        // If there were previously no callbacks, wakeup the runner thread.
        if self.input_callbacks.len() == 0 && self.output_callbacks.len() == 0 {
            if let Some(ref sender) = self.wakeup_sender {
                let _ = sender.send(());
            }
        }
        self.input_callbacks.push(Some(callbacks));
        self.input_callbacks.len() - 1
    }

    /// Removes the callbacks at specified index, returning them. Panics if the index is invalid
    /// (out of range or there is a None element at that position).
    fn remove_input_callbacks(&mut self, index: usize) -> InputCallbacks {
        let cbs = self.input_callbacks[index].take().unwrap();
        while self.input_callbacks.len() > 0
            && self.input_callbacks[self.input_callbacks.len() - 1].is_none()
        {
            self.input_callbacks.pop();
        }
        cbs
    }

    /// Send an error to all input and output error callbacks.
    fn error(&mut self, e: impl Into<StreamError>) {
        let e = e.into();
        for cbs in &mut self.input_callbacks {
            if let Some(cbs) = cbs {
                (cbs.error_callback)(e.clone());
            }
        }
        for cbs in &mut self.output_callbacks {
            if let Some(cbs) = cbs {
                (cbs.error_callback)(e.clone());
            }
        }
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
            sndio_sys::sio_getpar(self.hdl.unwrap(), par as *mut _)
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
        if self.hdl.is_none() {
            return Err(backend_specific_error(
                "cannot set params if device is not open",
            ));
        }
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
            // unwrap OK because of the check at the top of this function.
            sndio_sys::sio_setpar(self.hdl.unwrap(), &mut newpar as *mut _)
        };
        if status != 1 {
            return Err(backend_specific_error("failed to set parameters with sio_setpar").into());
        }
        Ok(())
    }
}

impl Drop for InnerState {
    fn drop(&mut self) {
        if let Some(hdl) = self.hdl.take() {
            unsafe {
                sndio_sys::sio_close(hdl);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

    pub fn set_xrun_behavior(&mut self, behavior: BufferXrunBehavior) {
        let mut inner_state = self.inner_state.lock().unwrap();
        inner_state.behavior = behavior;
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
        let mut inner_state = self.inner_state.lock().unwrap();

        if inner_state.sample_rate_to_par.is_none() {
            inner_state.open()?;
        }

        if inner_state.sample_rate_to_par.is_none() {
            return Err(backend_specific_error("no sample rate map!").into());
        }

        let mut config_ranges = vec![];
        // unwrap OK because of the check at the top of this function.
        for (_, par) in inner_state.sample_rate_to_par.as_ref().unwrap() {
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

    #[inline]
    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        let mut inner_state = self.inner_state.lock().unwrap();

        if inner_state.sample_rate_to_par.is_none() {
            inner_state.open()?;
        }

        if inner_state.sample_rate_to_par.is_none() {
            return Err(backend_specific_error("no sample rate map!").into());
        }

        let mut config_ranges = vec![];
        // unwrap OK because of the check at the top of this function.
        for (_, par) in inner_state.sample_rate_to_par.as_ref().unwrap() {
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

    #[inline]
    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut inner_state = self.inner_state.lock().unwrap();

        if inner_state.sample_rate_to_par.is_none() {
            inner_state.open()?;
        }

        // unwrap OK because the open call above will ensure this is not None.
        let config = if let Some(par) = inner_state
            .sample_rate_to_par
            .as_ref()
            .unwrap()
            .get(&DEFAULT_SAMPLE_RATE.0)
        {
            supported_config_from_par(par, par.rchan)
        } else {
            return Err(
                backend_specific_error("missing map of sample rates to sio_par structs!").into(),
            );
        };

        Ok(config)
    }

    #[inline]
    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let mut inner_state = self.inner_state.lock().unwrap();

        if inner_state.sample_rate_to_par.is_none() {
            inner_state.open()?;
        }

        // unwrap OK because the open call above will ensure this is not None.
        let config = if let Some(par) = inner_state
            .sample_rate_to_par
            .as_ref()
            .unwrap()
            .get(&DEFAULT_SAMPLE_RATE.0)
        {
            supported_config_from_par(par, par.pchan)
        } else {
            return Err(
                backend_specific_error("missing map of sample rates to sio_par structs!").into(),
            );
        };

        Ok(config)
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

        setup_stream(&mut inner_state, config)?;

        let boxed_data_cb = if sample_format != SampleFormat::I16 {
            input_adapter_callback(
                data_callback,
                inner_state.buffer_size.unwrap(), // unwrap OK because configured in setup_stream, above
                sample_format,
            )
        } else {
            Box::new(data_callback)
        };

        let idx = inner_state.add_input_callbacks(InputCallbacks {
            data_callback: boxed_data_cb,
            error_callback: Box::new(error_callback),
        });

        if inner_state.status != Status::Running {
            thread::spawn(move || runner(inner_state_arc));
            inner_state.status = Status::Running;
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

        let mut inner_state = self.inner_state.lock().unwrap();

        setup_stream(&mut inner_state, config)?;

        let boxed_data_cb = if sample_format != SampleFormat::I16 {
            output_adapter_callback(
                data_callback,
                inner_state.buffer_size.unwrap(), // unwrap OK because configured in setup_stream, above
                sample_format,
            )
        } else {
            Box::new(data_callback)
        };

        let idx = inner_state.add_output_callbacks(OutputCallbacks {
            data_callback: boxed_data_cb,
            error_callback: Box::new(error_callback),
        });

        if inner_state.status != Status::Running {
            thread::spawn(move || runner(inner_state_arc));
            inner_state.status = Status::Running;
        }

        drop(inner_state); // Unlock
        Ok(Stream {
            inner_state: self.inner_state.clone(),
            is_output: true,
            index: idx,
        })
    }
}

/// Common code shared between build_input_stream_raw and build_output_stream_raw
fn setup_stream(
    inner_state: &mut InnerState,
    config: &StreamConfig,
) -> Result<(), BuildStreamError> {
    if inner_state.sample_rate_to_par.is_none() {
        inner_state.open()?;
    }

    // TODO: one day we should be able to remove this
    assert_eq!(
        inner_state.input_callbacks.len() + inner_state.output_callbacks.len() > 0,
        inner_state.par.is_some(),
        "par can be None if and only if there are no input or output callbacks"
    );

    let par; // Either the currently configured par for existing streams or the one we will set
    if let Some(configured_par) = inner_state.par {
        par = configured_par;

        // Perform some checks
        if par.rate != config.sample_rate.0 as u32 {
            return Err(backend_specific_error("sample rates don't match").into());
        }
    } else {
        // No running streams yet; we get to set the par.
        // unwrap OK because this is setup on inner_state.open() call above
        if let Some(par_) = inner_state
            .sample_rate_to_par
            .as_ref()
            .unwrap()
            .get(&config.sample_rate.0)
        {
            par = par_.clone();
        } else {
            return Err(backend_specific_error(format!(
                "no configuration for sample rate {}",
                config.sample_rate.0
            ))
            .into());
        }
    }

    // Round up the buffer size the user selected to the next multiple of par.round. If there
    // was already a stream created with a different buffer size, return an error (sorry).
    // Note: if we want stereo support, this will need to change.
    let round = par.round as usize;
    let desired_buffer_size = match config.buffer_size {
        BufferSize::Fixed(requested) => {
            if requested > 0 {
                requested as usize + round - ((requested - 1) as usize % round) - 1
            } else {
                round
            }
        }
        BufferSize::Default => {
            if let Some(bufsize) = inner_state.buffer_size {
                bufsize
            } else {
                DEFAULT_ROUND_MULTIPLE * round
            }
        }
    };

    if inner_state.buffer_size.is_some() && inner_state.buffer_size != Some(desired_buffer_size) {
        return Err(backend_specific_error("buffer sizes don't match").into());
    }

    if inner_state.par.is_none() {
        let mut par = par;
        par.appbufsz = desired_buffer_size as u32;
        inner_state.buffer_size = Some(desired_buffer_size);
        inner_state.set_params(&par)?;
        inner_state.par = Some(par.clone());
    }
    Ok(())
}

fn supported_config_from_par(par: &sndio_sys::sio_par, num_channels: u32) -> SupportedStreamConfig {
    SupportedStreamConfig {
        channels: num_channels as u16,
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
            inner_state.remove_output_callbacks(self.index);
        } else {
            inner_state.remove_input_callbacks(self.index);
        }

        if inner_state.input_callbacks.len() == 0
            && inner_state.output_callbacks.len() == 0
            && inner_state.status == Status::Running
        {
            if let Some(ref sender) = inner_state.wakeup_sender {
                let _ = sender.send(());
            }
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let inner_state = self.inner_state.lock().unwrap();
        if inner_state.input_callbacks.len() == 0
            && inner_state.output_callbacks.len() == 0
            && inner_state.status == Status::Running
        {
            // Attempt to wakeup runner thread
            if let Some(ref sender) = inner_state.wakeup_sender {
                let _ = sender.send(());
            }
        }
    }
}
