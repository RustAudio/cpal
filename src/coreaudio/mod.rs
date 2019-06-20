extern crate coreaudio;
extern crate core_foundation_sys;

use ChannelCount;
use BackendSpecificError;
use BuildStreamError;
use DefaultFormatError;
use DeviceNameError;
use Format;
use SupportedFormatsError;
use Sample;
use SampleFormat;
use SampleRate;
use StreamData;
use SupportedFormat;
use UnknownTypeInputBuffer;
use UnknownTypeOutputBuffer;

use std::ffi::CStr;
use std::fmt;
use std::mem;
use std::os::raw::c_char;
use std::ptr::null;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::slice;

use self::coreaudio::audio_unit::{AudioUnit, Scope, Element};
use self::coreaudio::audio_unit::render_callback::{self, data};
use self::coreaudio::sys::{
    AudioBuffer,
    AudioBufferList,
    AudioDeviceID,
    AudioObjectAddPropertyListener,
    AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize,
    AudioObjectID,
    AudioObjectPropertyAddress,
    AudioObjectPropertyScope,
    AudioObjectRemovePropertyListener,
    AudioObjectSetPropertyData,
    AudioStreamBasicDescription,
    AudioValueRange,
    kAudioDevicePropertyAvailableNominalSampleRates,
    kAudioDevicePropertyDeviceNameCFString,
    kAudioDevicePropertyNominalSampleRate,
    kAudioObjectPropertyScopeInput,
    kAudioObjectPropertyScopeGlobal,
    kAudioDevicePropertyScopeOutput,
    kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreamFormat,
    kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsPacked,
    kAudioFormatLinearPCM,
    kAudioHardwareNoError,
    kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeOutput,
    kAudioOutputUnitProperty_CurrentDevice,
    kAudioOutputUnitProperty_EnableIO,
    kAudioUnitProperty_StreamFormat,
    kCFStringEncodingUTF8,
    OSStatus,
};
use self::core_foundation_sys::string::{
    CFStringRef,
    CFStringGetCStringPtr,
};

mod enumerate;

pub use self::enumerate::{Devices, SupportedInputFormats, SupportedOutputFormats, default_input_device, default_output_device};

#[derive(Clone, PartialEq, Eq)]
pub struct Device {
    audio_device_id: AudioDeviceID,
}

impl Device {
    pub fn name(&self) -> Result<String, DeviceNameError> {
        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyDeviceNameCFString,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMaster,
        };
        let device_name: CFStringRef = null();
        let data_size = mem::size_of::<CFStringRef>();
        let c_str = unsafe {
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                &device_name as *const _ as *mut _,
            );
            check_os_status(status)?;

            let c_string: *const c_char = CFStringGetCStringPtr(device_name, kCFStringEncodingUTF8);
            if c_string == null() {
                let description = "core foundation unexpectedly returned null string".to_string();
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
            CStr::from_ptr(c_string as *mut _)
        };
        Ok(c_str.to_string_lossy().into_owned())
    }

    // Logic re-used between `supported_input_formats` and `supported_output_formats`.
    fn supported_formats(
        &self,
        scope: AudioObjectPropertyScope,
    ) -> Result<SupportedOutputFormats, SupportedFormatsError>
    {
        let mut property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMaster,
        };

        unsafe {
            // Retrieve the devices audio buffer list.
            let data_size = 0u32;
            let status = AudioObjectGetPropertyDataSize(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
            );
            check_os_status(status)?;

            let mut audio_buffer_list: Vec<u8> = vec![];
            audio_buffer_list.reserve_exact(data_size as usize);
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                audio_buffer_list.as_mut_ptr() as *mut _,
            );
            check_os_status(status)?;

            let audio_buffer_list = audio_buffer_list.as_mut_ptr() as *mut AudioBufferList;

            // If there's no buffers, skip.
            if (*audio_buffer_list).mNumberBuffers == 0 {
                return Ok(vec![].into_iter());
            }

            // Count the number of channels as the sum of all channels in all output buffers.
            let n_buffers = (*audio_buffer_list).mNumberBuffers as usize;
            let first: *const AudioBuffer = (*audio_buffer_list).mBuffers.as_ptr();
            let buffers: &'static [AudioBuffer] = slice::from_raw_parts(first, n_buffers);
            let mut n_channels = 0;
            for buffer in buffers {
                n_channels += buffer.mNumberChannels as usize;
            }

            // AFAIK the sample format should always be f32 on macos and i16 on iOS? Feel free to
            // fix this if more pcm formats are supported.
            let sample_format = if cfg!(target_os = "ios") {
                SampleFormat::I16
            } else {
                SampleFormat::F32
            };

            // Get available sample rate ranges.
            property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
            let data_size = 0u32;
            let status = AudioObjectGetPropertyDataSize(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
            );
            check_os_status(status)?;

            let n_ranges = data_size as usize / mem::size_of::<AudioValueRange>();
            let mut ranges: Vec<u8> = vec![];
            ranges.reserve_exact(data_size as usize);
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                ranges.as_mut_ptr() as *mut _,
            );
            check_os_status(status)?;

            let ranges: *mut AudioValueRange = ranges.as_mut_ptr() as *mut _;
            let ranges: &'static [AudioValueRange] = slice::from_raw_parts(ranges, n_ranges);

            // Collect the supported formats for the device.
            let mut fmts = vec![];
            for range in ranges {
                let fmt = SupportedFormat {
                    channels: n_channels as ChannelCount,
                    min_sample_rate: SampleRate(range.mMinimum as _),
                    max_sample_rate: SampleRate(range.mMaximum as _),
                    data_type: sample_format,
                };
                fmts.push(fmt);
            }

            Ok(fmts.into_iter())
        }
    }

    pub fn supported_input_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        self.supported_formats(kAudioObjectPropertyScopeInput)
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        self.supported_formats(kAudioObjectPropertyScopeOutput)
    }

    fn default_format(
        &self,
        scope: AudioObjectPropertyScope,
    ) -> Result<Format, DefaultFormatError>
    {
        fn default_format_error_from_os_status(status: OSStatus) -> Option<DefaultFormatError> {
            let err = match coreaudio::Error::from_os_status(status) {
                Err(err) => err,
                Ok(_) => return None,
            };
            match err {
                coreaudio::Error::RenderCallbackBufferFormatDoesNotMatchAudioUnitStreamFormat |
                coreaudio::Error::NoKnownSubtype |
                coreaudio::Error::AudioUnit(coreaudio::error::AudioUnitError::FormatNotSupported) |
                coreaudio::Error::AudioCodec(_) |
                coreaudio::Error::AudioFormat(_) => Some(DefaultFormatError::StreamTypeNotSupported),
                _ => Some(DefaultFormatError::DeviceNotAvailable),
            }
        }

        let property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamFormat,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMaster,
        };

        unsafe {
            let asbd: AudioStreamBasicDescription = mem::uninitialized();
            let data_size = mem::size_of::<AudioStreamBasicDescription>() as u32;
            let status = AudioObjectGetPropertyData(
                self.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                &asbd as *const _ as *mut _,
            );

            if status != kAudioHardwareNoError as i32 {
                let err = default_format_error_from_os_status(status)
                    .expect("no known error for OSStatus");
                return Err(err);
            }

            let sample_format = {
                let audio_format = coreaudio::audio_unit::AudioFormat::from_format_and_flag(
                    asbd.mFormatID,
                    Some(asbd.mFormatFlags),
                );
                let flags = match audio_format {
                    Some(coreaudio::audio_unit::AudioFormat::LinearPCM(flags)) => flags,
                    _ => return Err(DefaultFormatError::StreamTypeNotSupported),
                };
                let maybe_sample_format =
                    coreaudio::audio_unit::SampleFormat::from_flags_and_bytes_per_frame(
                        flags,
                        asbd.mBytesPerFrame,
                    );
                match maybe_sample_format {
                    Some(coreaudio::audio_unit::SampleFormat::F32) => SampleFormat::F32,
                    Some(coreaudio::audio_unit::SampleFormat::I16) => SampleFormat::I16,
                    _ => return Err(DefaultFormatError::StreamTypeNotSupported),
                }
            };

            let format = Format {
                sample_rate: SampleRate(asbd.mSampleRate as _),
                channels: asbd.mChannelsPerFrame as _,
                data_type: sample_format,
            };
            Ok(format)
        }
    }

    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        self.default_format(kAudioObjectPropertyScopeInput)
    }

    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        self.default_format(kAudioObjectPropertyScopeOutput)
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("audio_device_id", &self.audio_device_id)
            .field("name", &self.name())
            .finish()
    }
}

// The ID of a stream is its index within the `streams` array of the events loop.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(usize);

pub struct EventLoop {
    // This `Arc` is shared with all the callbacks of coreaudio.
    active_callbacks: Arc<ActiveCallbacks>,
    streams: Mutex<Vec<Option<StreamInner>>>,
}

struct ActiveCallbacks {
    // Whenever the `run()` method is called with a callback, this callback is put in this list.
    callbacks: Mutex<Vec<&'static mut (FnMut(StreamId, StreamData) + Send)>>,
}

struct StreamInner {
    playing: bool,
    audio_unit: AudioUnit,
    // Track the device with which the audio unit was spawned.
    //
    // We must do this so that we can avoid changing the device sample rate if there is already
    // a stream associated with the device.
    device_id: AudioDeviceID,
}

// TODO need stronger error identification
impl From<coreaudio::Error> for BuildStreamError {
    fn from(err: coreaudio::Error) -> BuildStreamError {
        match err {
            coreaudio::Error::RenderCallbackBufferFormatDoesNotMatchAudioUnitStreamFormat |
            coreaudio::Error::NoKnownSubtype |
            coreaudio::Error::AudioUnit(coreaudio::error::AudioUnitError::FormatNotSupported) |
            coreaudio::Error::AudioCodec(_) |
            coreaudio::Error::AudioFormat(_) => BuildStreamError::FormatNotSupported,
            _ => BuildStreamError::DeviceNotAvailable,
        }
    }
}

// Create a coreaudio AudioStreamBasicDescription from a CPAL Format.
fn asbd_from_format(format: &Format) -> AudioStreamBasicDescription {
    let n_channels = format.channels as usize;
    let sample_rate = format.sample_rate.0;
    let bytes_per_channel = format.data_type.sample_size();
    let bits_per_channel = bytes_per_channel * 8;
    let bytes_per_frame = n_channels * bytes_per_channel;
    let frames_per_packet = 1;
    let bytes_per_packet = frames_per_packet * bytes_per_frame;
    let sample_format = format.data_type;
    let format_flags = match sample_format {
        SampleFormat::F32 => (kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked) as u32,
        _ => kAudioFormatFlagIsPacked as u32,
    };
    let asbd = AudioStreamBasicDescription {
        mBitsPerChannel: bits_per_channel as _,
        mBytesPerFrame: bytes_per_frame as _,
        mChannelsPerFrame: n_channels as _,
        mBytesPerPacket: bytes_per_packet as _,
        mFramesPerPacket: frames_per_packet as _,
        mFormatFlags: format_flags,
        mFormatID: kAudioFormatLinearPCM,
        mSampleRate: sample_rate as _,
        ..Default::default()
    };
    asbd
}

fn audio_unit_from_device(device: &Device, input: bool) -> Result<AudioUnit, coreaudio::Error> {
    let mut audio_unit = {
        let au_type = if cfg!(target_os = "ios") {
            // The HalOutput unit isn't available in iOS unfortunately.
            // RemoteIO is a sensible replacement.
            // See https://goo.gl/CWwRTx
            coreaudio::audio_unit::IOType::RemoteIO
        } else {
            coreaudio::audio_unit::IOType::HalOutput
        };
        AudioUnit::new(au_type)?
    };

    if input {
        // Enable input processing.
        let enable_input = 1u32;
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Input,
            Element::Input,
            Some(&enable_input),
        )?;

        // Disable output processing.
        let disable_output = 0u32;
        audio_unit.set_property(
            kAudioOutputUnitProperty_EnableIO,
            Scope::Output,
            Element::Output,
            Some(&disable_output),
        )?;
    }

    audio_unit.set_property(
        kAudioOutputUnitProperty_CurrentDevice,
        Scope::Global,
        Element::Output,
        Some(&device.audio_device_id),
    )?;

    Ok(audio_unit)
}

impl EventLoop {
    #[inline]
    pub fn new() -> EventLoop {
        EventLoop {
            active_callbacks: Arc::new(ActiveCallbacks { callbacks: Mutex::new(Vec::new()) }),
            streams: Mutex::new(Vec::new()),
        }
    }

    #[inline]
    pub fn run<F>(&self, mut callback: F) -> !
        where F: FnMut(StreamId, StreamData) + Send
    {
        {
            let callback: &mut (FnMut(StreamId, StreamData) + Send) = &mut callback;
            self.active_callbacks
                .callbacks
                .lock()
                .unwrap()
                .push(unsafe { mem::transmute(callback) });
        }

        loop {
            // So the loop does not get optimised out in --release
            thread::sleep(Duration::new(1u64, 0u32));
        }

        // Note: if we ever change this API so that `run` can return, then it is critical that
        // we remove the callback from `active_callbacks`.
    }

    fn next_stream_id(&self) -> usize {
        let streams_lock = self.streams.lock().unwrap();
        let stream_id = streams_lock
            .iter()
            .position(|n| n.is_none())
            .unwrap_or(streams_lock.len());
        stream_id
    }

    // Add the stream to the list of streams within `self`.
    fn add_stream(&self, stream_id: usize, au: AudioUnit, device_id: AudioDeviceID) {
        let inner = StreamInner {
            playing: true,
            audio_unit: au,
            device_id: device_id,
        };

        let mut streams_lock = self.streams.lock().unwrap();
        if stream_id == streams_lock.len() {
            streams_lock.push(Some(inner));
        } else {
            streams_lock[stream_id] = Some(inner);
        }
    }

    #[inline]
    pub fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        // The scope and element for working with a device's input stream.
        let scope = Scope::Output;
        let element = Element::Input;

        // Check whether or not we need to change the device sample rate to suit the one specified for the stream.
        unsafe {
            // Get the current sample rate.
            let mut property_address = AudioObjectPropertyAddress {
                mSelector: kAudioDevicePropertyNominalSampleRate,
	        mScope: kAudioObjectPropertyScopeGlobal,
	        mElement: kAudioObjectPropertyElementMaster,
            };
            let sample_rate: f64 = 0.0;
            let data_size = mem::size_of::<f64>() as u32;
            let status = AudioObjectGetPropertyData(
                device.audio_device_id,
                &property_address as *const _,
                0,
                null(),
                &data_size as *const _ as *mut _,
                &sample_rate as *const _ as *mut _,
            );
            coreaudio::Error::from_os_status(status)?;

            // If the requested sample rate is different to the device sample rate, update the device.
            if sample_rate as u32 != format.sample_rate.0 {

                // In order to avoid breaking existing input streams we `panic!` if there is already an
                // active input stream for this device with the actual sample rate.
                for stream in &*self.streams.lock().unwrap() {
                    if let Some(stream) = stream.as_ref() {
                        if stream.device_id == device.audio_device_id {
                            panic!("cannot change device sample rate for stream as an existing stream \
                                    is already running at the current sample rate.");
                        }
                    }
                }

                // Get available sample rate ranges.
                property_address.mSelector = kAudioDevicePropertyAvailableNominalSampleRates;
                let data_size = 0u32;
                let status = AudioObjectGetPropertyDataSize(
                    device.audio_device_id,
                    &property_address as *const _,
                    0,
                    null(),
                    &data_size as *const _ as *mut _,
                );
                coreaudio::Error::from_os_status(status)?;
                let n_ranges = data_size as usize / mem::size_of::<AudioValueRange>();
                let mut ranges: Vec<u8> = vec![];
                ranges.reserve_exact(data_size as usize);
                let status = AudioObjectGetPropertyData(
                    device.audio_device_id,
                    &property_address as *const _,
                    0,
                    null(),
                    &data_size as *const _ as *mut _,
                    ranges.as_mut_ptr() as *mut _,
                );
                coreaudio::Error::from_os_status(status)?;
                let ranges: *mut AudioValueRange = ranges.as_mut_ptr() as *mut _;
                let ranges: &'static [AudioValueRange] = slice::from_raw_parts(ranges, n_ranges);

                // Now that we have the available ranges, pick the one matching the desired rate.
                let sample_rate = format.sample_rate.0;
                let maybe_index = ranges
                    .iter()
                    .position(|r| r.mMinimum as u32 == sample_rate && r.mMaximum as u32 == sample_rate);
                let range_index = match maybe_index {
                    None => return Err(BuildStreamError::FormatNotSupported),
                    Some(i) => i,
                };

                // Update the property selector to specify the nominal sample rate.
                property_address.mSelector = kAudioDevicePropertyNominalSampleRate;

                // Setting the sample rate of a device is an asynchronous process in coreaudio.
                //
                // Thus we are required to set a `listener` so that we may be notified when the
                // change occurs.
                unsafe extern "C" fn rate_listener(
                    device_id: AudioObjectID,
                    _n_addresses: u32,
                    _properties: *const AudioObjectPropertyAddress,
                    rate_ptr: *mut ::std::os::raw::c_void,
                ) -> OSStatus {
                    let rate_ptr: *const f64 = rate_ptr as *const _;
                    let data_size = mem::size_of::<f64>();
                    let property_address = AudioObjectPropertyAddress {
                        mSelector: kAudioDevicePropertyNominalSampleRate,
	                mScope: kAudioObjectPropertyScopeGlobal,
	                mElement: kAudioObjectPropertyElementMaster,
                    };
                    AudioObjectGetPropertyData(
                        device_id,
                        &property_address as *const _,
                        0,
                        null(),
                        &data_size as *const _ as *mut _,
                        rate_ptr as *const _ as *mut _,
                    )
                }

                // Add our sample rate change listener callback.
                let reported_rate: f64 = 0.0;
                let status = AudioObjectAddPropertyListener(
                    device.audio_device_id,
                    &property_address as *const _,
                    Some(rate_listener),
                    &reported_rate as *const _ as *mut _,
                );
                coreaudio::Error::from_os_status(status)?;

                // Finally, set the sample rate.
                let sample_rate = sample_rate as f64;
                let status = AudioObjectSetPropertyData(
                    device.audio_device_id,
                    &property_address as *const _,
                    0,
                    null(),
                    data_size,
                    &ranges[range_index] as *const _ as *const _,
                );
                coreaudio::Error::from_os_status(status)?;

                // Wait for the reported_rate to change.
                //
                // This should not take longer than a few ms, but we timeout after 1 sec just in case.
                let timer = ::std::time::Instant::now();
                while sample_rate != reported_rate {
                    if timer.elapsed() > ::std::time::Duration::from_secs(1) {
                        panic!("timeout waiting for sample rate update for device");
                    }
                    ::std::thread::sleep(::std::time::Duration::from_millis(5));
                }

                // Remove the `rate_listener` callback.
                let status = AudioObjectRemovePropertyListener(
                    device.audio_device_id,
                    &property_address as *const _,
                    Some(rate_listener),
                    &reported_rate as *const _ as *mut _,
                );
                coreaudio::Error::from_os_status(status)?;
            }
        }

        let mut audio_unit = audio_unit_from_device(device, true)?;

        // Set the stream in interleaved mode.
        let asbd = asbd_from_format(format);
        audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

        // Determine the future ID of the stream.
        let stream_id = self.next_stream_id();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let active_callbacks = self.active_callbacks.clone();
        let sample_format = format.data_type;
        let bytes_per_channel = format.data_type.sample_size();
        type Args = render_callback::Args<data::Raw>;
        audio_unit.set_input_callback(move |args: Args| unsafe {
            let ptr = (*args.data.data).mBuffers.as_ptr() as *const AudioBuffer;
            let len = (*args.data.data).mNumberBuffers as usize;
            let buffers: &[AudioBuffer] = slice::from_raw_parts(ptr, len);

            // TODO: Perhaps loop over all buffers instead?
            let AudioBuffer {
                mNumberChannels: _num_channels,
                mDataByteSize: data_byte_size,
                mData: data
            } = buffers[0];

            let mut callbacks = active_callbacks.callbacks.lock().unwrap();

            // A small macro to simplify handling the callback for different sample types.
            macro_rules! try_callback {
                ($SampleFormat:ident, $SampleType:ty) => {{
                    let data_len = (data_byte_size as usize / bytes_per_channel) as usize;
                    let data_slice = slice::from_raw_parts(data as *const $SampleType, data_len);
                    let callback = match callbacks.get_mut(0) {
                        Some(cb) => cb,
                        None => return Ok(()),
                    };
                    let unknown_type_buffer = UnknownTypeInputBuffer::$SampleFormat(::InputBuffer { buffer: data_slice });
                    let stream_data = StreamData::Input { buffer: unknown_type_buffer };
                    callback(StreamId(stream_id), stream_data);
                }};
            }

            match sample_format {
                SampleFormat::F32 => try_callback!(F32, f32),
                SampleFormat::I16 => try_callback!(I16, i16),
                SampleFormat::U16 => try_callback!(U16, u16),
            }

            Ok(())
        })?;

        // TODO: start playing now? is that consistent with the other backends?
        audio_unit.start()?;

        // Add the stream to the list of streams within `self`.
        self.add_stream(stream_id, audio_unit, device.audio_device_id);

        Ok(StreamId(stream_id))
    }

    #[inline]
    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError>
    {
        let mut audio_unit = audio_unit_from_device(device, false)?;

        // The scope and element for working with a device's output stream.
        let scope = Scope::Input;
        let element = Element::Output;

        // Set the stream in interleaved mode.
        let asbd = asbd_from_format(format);
        audio_unit.set_property(kAudioUnitProperty_StreamFormat, scope, element, Some(&asbd))?;

        // Determine the future ID of the stream.
        let stream_id = self.next_stream_id();

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let active_callbacks = self.active_callbacks.clone();
        let sample_format = format.data_type;
        let bytes_per_channel = format.data_type.sample_size();
        type Args = render_callback::Args<data::Raw>;
        audio_unit.set_render_callback(move |args: Args| unsafe {
            // If `run()` is currently running, then a callback will be available from this list.
            // Otherwise, we just fill the buffer with zeroes and return.

            let AudioBuffer {
                mNumberChannels: _num_channels,
                mDataByteSize: data_byte_size,
                mData: data
            } = (*args.data.data).mBuffers[0];

            let mut callbacks = active_callbacks.callbacks.lock().unwrap();

            // A small macro to simplify handling the callback for different sample types.
            macro_rules! try_callback {
                ($SampleFormat:ident, $SampleType:ty, $equilibrium:expr) => {{
                    let data_len = (data_byte_size as usize / bytes_per_channel) as usize;
                    let data_slice = slice::from_raw_parts_mut(data as *mut $SampleType, data_len);
                    let callback = match callbacks.get_mut(0) {
                        Some(cb) => cb,
                        None => {
                            for sample in data_slice.iter_mut() {
                                *sample = $equilibrium;
                            }
                            return Ok(());
                        }
                    };
                    let unknown_type_buffer = UnknownTypeOutputBuffer::$SampleFormat(::OutputBuffer { buffer: data_slice });
                    let stream_data = StreamData::Output { buffer: unknown_type_buffer };
                    callback(StreamId(stream_id), stream_data);
                }};
            }

            match sample_format {
                SampleFormat::F32 => try_callback!(F32, f32, 0.0),
                SampleFormat::I16 => try_callback!(I16, i16, 0),
                SampleFormat::U16 => try_callback!(U16, u16, ::std::u16::MAX / 2),
            }

            Ok(())
        })?;

        // TODO: start playing now? is that consistent with the other backends?
        audio_unit.start()?;

        // Add the stream to the list of streams within `self`.
        self.add_stream(stream_id, audio_unit, device.audio_device_id);

        Ok(StreamId(stream_id))
    }

    pub fn destroy_stream(&self, stream_id: StreamId) {
        let mut streams = self.streams.lock().unwrap();
        streams[stream_id.0] = None;
    }

    pub fn play_stream(&self, stream: StreamId) {
        let mut streams = self.streams.lock().unwrap();
        let stream = streams[stream.0].as_mut().unwrap();

        if !stream.playing {
            stream.audio_unit.start().unwrap();
            stream.playing = true;
        }
    }

    pub fn pause_stream(&self, stream: StreamId) {
        let mut streams = self.streams.lock().unwrap();
        let stream = streams[stream.0].as_mut().unwrap();

        if stream.playing {
            stream.audio_unit.stop().unwrap();
            stream.playing = false;
        }
    }
}

fn check_os_status(os_status: OSStatus) -> Result<(), BackendSpecificError> {
    match coreaudio::Error::from_os_status(os_status) {
        Ok(()) => Ok(()),
        Err(err) => {
            let description = std::error::Error::description(&err).to_string();
            Err(BackendSpecificError { description })
        }
    }
}
