use crate::ErrorKind::BackendError;
use crate::{DeviceDirection, SampleFormat, StreamConfig, SupportedBufferSize, SupportedStreamConfig};
use std::mem::MaybeUninit;
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    sync::atomic::{AtomicU8, Ordering},
    ops::{Deref, DerefMut},
    time::Duration,
    mem::transmute,
    fmt::Debug,
    slice,
};
use tap::Pipe;
use wasapi::{Direction, IAudioClient, SampleType, WaveFormat};
use windows::Win32::Foundation::{CloseHandle, E_POINTER, HANDLE, WAIT_EVENT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::{ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject};
use windows_core::{Interface, PCWSTR, Param, Ref, implement};

pub type CpalError     = crate::Error;
pub type WasapiRsError = wasapi::WasapiError;
pub type WindowsError  = windows_core::Error;

pub struct ScratchBuffer {
    pub ptr: *mut u8,
    pub layout: Layout
}

unsafe impl Send for ScratchBuffer {}
unsafe impl Sync for ScratchBuffer {}

impl ScratchBuffer {
    pub fn new(sample_count: usize, format: SampleFormat) -> Self {
        let size   = sample_count * format.sample_size();
        let align  = format.sample_size();
        let layout = Layout::from_size_align(size, align).unwrap();
        let ptr    = unsafe { alloc_zeroed(layout) };

        Self { ptr, layout }
    }

    pub fn slice(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self.ptr,
                self.layout.size()
            )
        }
    }

    fn slice_i32(&mut self) -> &mut [i32] {
        unsafe {
            slice::from_raw_parts_mut(
                self.ptr.cast(),
                self.layout.size() / size_of::<i32>()
            )
        }
    }

    pub fn shift_up(&mut self, sample_count: usize) {
        for sample in &mut self.slice_i32()[..sample_count] {
            *sample <<= 8;
        }
    }

    pub fn shift_down(&mut self, sample_count: usize) {
        for sample in &mut self.slice_i32()[..sample_count] {
            *sample >>= 8;
        }
    }
}

impl Drop for ScratchBuffer {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr, self.layout) };
    }
}

#[derive(Debug)]
pub struct Event(HANDLE);

unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    pub fn new() -> Result<Self, WindowsError> {
        unsafe { CreateEventW(None, false, false, None) }.map(Self)
    }

    pub const fn as_raw(&self) -> HANDLE {
        self.0
    }

    pub fn set_signaled(&self) -> Result<(), WindowsError> {
        unsafe { SetEvent(self.0) }
    }

    pub fn wait_single(&self, timeout: Option<Duration>) -> Result<bool, WindowsError> {
        let millis = timeout.map_or(INFINITE, |duration| duration.as_millis() as _);

        match unsafe { WaitForSingleObject(self.0, millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(windows_core::Error::from_thread()),
            _          => unreachable!()
        }
    }

    pub fn wait_multiple(events: &[HANDLE], timeout: Option<Duration>) -> Result<Option<usize>, WindowsError> {
        let dwmilliseconds = timeout.map_or(INFINITE, |duration| duration.as_millis() as _);
        let wait_event = unsafe { WaitForMultipleObjects(events, false, dwmilliseconds) };

        #[deny(nonstandard_style, reason = "prevent accidental wildcards")]
        match wait_event {
            WAIT_FAILED => Err(windows_core::Error::from_thread()),
            WAIT_TIMEOUT => Ok(None),
            WAIT_EVENT(i) => Ok(Some(i as _))
        }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        _ = unsafe { CloseHandle(self.0) };
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamState {
    Running,
    Draining,
    Halted,
    Exit
}

#[derive(Debug)]
pub struct WaitableStreamState {
    atomic: AtomicU8,
    event: Event
}

impl WaitableStreamState {
    pub fn new() -> Result<Self, WindowsError> {
        Self {
            atomic: AtomicU8::new(StreamState::Halted as _),
            event: Event::new()?
        }
        .pipe(Ok)
    }

    #[must_use]
    pub fn get(&self) -> StreamState {
        unsafe { transmute(self.atomic.load(Ordering::Relaxed)) }
    }

    pub fn set(&self, value: StreamState, signal: bool) -> Result<(), WindowsError> {
        self.atomic.store(value as _, Ordering::Relaxed);
        if signal {
            self.event.set_signaled()?;
        }
        Ok(())
    }

    #[must_use]
    pub const fn event(&self) -> &Event {
        &self.event
    }
}

impl From<WasapiRsError> for CpalError {
    fn from(value: WasapiRsError) -> Self {
        CpalError::with_message(BackendError, format!("{value}"))
    }
}

impl From<Direction> for DeviceDirection {
    fn from(value: Direction) -> Self {
        match value {
            Direction::Capture => DeviceDirection::Input,
            Direction::Render  => DeviceDirection::Output
        }
    }
}

impl TryFrom<WaveFormat> for SupportedStreamConfig {
    type Error = CpalError;

    fn try_from(waveformat: WaveFormat) -> Result<Self, CpalError> {
        SupportedStreamConfig {
            channels: waveformat.get_nchannels(),
            sample_rate: waveformat.get_samplespersec(),
            buffer_size: SupportedBufferSize::default(),
            sample_format: match waveformat.get_validbitspersample() {
                 8 => SampleFormat::I8,
                16 => SampleFormat::I16,
                24 => SampleFormat::I24,
                32 if waveformat.get_subformat()? == SampleType::Float => SampleFormat::F32,
                32 => SampleFormat::I32,
                _  => return Err(CpalError::with_message(BackendError, "sound server reported unexpected mix format"))
            }
        }
        .pipe(Ok)
    }
}

pub fn create_waveformat(config: StreamConfig, format: SampleFormat) -> Result<WaveFormat, WasapiRsError> {
    let sample_type =
        if      format.is_int()   { SampleType::Int }
        else if format.is_float() { SampleType::Float }
        else                      { return Err(WasapiRsError::UnsupportedFormat) };

    WaveFormat::new(
        format.sample_size() * u8::BITS as usize,
        format.bits_per_sample() as _,
        sample_type,
        config.sample_rate as _,
        config.channels as _,
        None
    )
    .pipe(Ok)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SendSync<T>(pub T);

unsafe impl<T> Send for SendSync<T> {}
unsafe impl<T> Sync for SendSync<T> {}

impl<T> Deref for SendSync<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for SendSync<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub fn activate_audio_interface(id: impl Param<PCWSTR>, timeout: Option<Duration>) -> Result<IAudioClient, WindowsError> {
    let event = Event::new()?;

    let operation = event
        .as_raw()
        .pipe(CompletionHandler)
        .pipe(IActivateAudioInterfaceCompletionHandler::from)
        .pipe(|handler| unsafe { ActivateAudioInterfaceAsync(id, &IAudioClient::IID, None, &handler) })?;

    event.wait_single(timeout)?;

    let mut hresult  = MaybeUninit::uninit();
    let mut iunknown = MaybeUninit::uninit();

    unsafe {
        operation.GetActivateResult(hresult.as_mut_ptr(), iunknown.as_mut_ptr())?;
        hresult .assume_init().ok()?;
        iunknown.assume_init().ok_or(E_POINTER)?.cast()
    }
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct CompletionHandler(HANDLE);

impl IActivateAudioInterfaceCompletionHandler_Impl for CompletionHandler_Impl {
    fn ActivateCompleted(&self, _activateoperation: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows_core::Result<()> {
        unsafe { SetEvent(self.0) }
    }
}