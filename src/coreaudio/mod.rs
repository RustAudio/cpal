extern crate coreaudio;
extern crate core_foundation_sys;

use ChannelCount;
use CreationError;
use DefaultFormatError;
use Format;
use FormatsEnumerationError;
use Sample;
use SampleFormat;
use SampleRate;
use StreamData;
use SupportedFormat;
use UnknownTypeOutputBuffer;

use std::ffi::CStr;
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
    AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize,
    AudioObjectPropertyAddress,
    AudioStreamBasicDescription,
    AudioValueRange,
    kAudioDevicePropertyAvailableNominalSampleRates,
    kAudioDevicePropertyDeviceNameCFString,
    kAudioDevicePropertyScopeOutput,
    kAudioDevicePropertyStreamConfiguration,
    kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsPacked,
    kAudioFormatLinearPCM,
    kAudioHardwareNoError,
    kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeOutput,
    kAudioOutputUnitProperty_CurrentDevice,
    kAudioUnitProperty_StreamFormat,
    kCFStringEncodingUTF8,
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
    pub fn name(&self) -> String {
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
            if status != kAudioHardwareNoError as i32 {
                return format!("<OSStatus: {:?}>", status);
            }
            let c_string: *const c_char = CFStringGetCStringPtr(device_name, kCFStringEncodingUTF8);
            if c_string == null() {
                return "<null>".into();
            }
            CStr::from_ptr(c_string as *mut _)
        };
        c_str.to_string_lossy().into_owned()
    }

    pub fn supported_input_formats(&self) -> Result<SupportedOutputFormats, FormatsEnumerationError> {
        unimplemented!();
    }

    pub fn supported_output_formats(&self) -> Result<SupportedOutputFormats, FormatsEnumerationError> {
        let mut property_address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioObjectPropertyScopeOutput,
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
            if status != kAudioHardwareNoError as i32 {
                unimplemented!();
            }
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
            if status != kAudioHardwareNoError as i32 {
                unimplemented!();
            }
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
            if status != kAudioHardwareNoError as i32 {
                unimplemented!();
            }
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
            if status != kAudioHardwareNoError as i32 {
                unimplemented!();
            }
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

    pub fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!();
    }

    pub fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!();
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
}

// TODO need stronger error identification
impl From<coreaudio::Error> for CreationError {
    fn from(err: coreaudio::Error) -> CreationError {
        match err {
            coreaudio::Error::RenderCallbackBufferFormatDoesNotMatchAudioUnitStreamFormat |
            coreaudio::Error::NoKnownSubtype |
            coreaudio::Error::AudioUnit(coreaudio::error::AudioUnitError::FormatNotSupported) |
            coreaudio::Error::AudioCodec(_) |
            coreaudio::Error::AudioFormat(_) => CreationError::FormatNotSupported,
            _ => CreationError::DeviceNotAvailable,
        }
    }
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
        let callback: &mut (FnMut(StreamId, StreamData) + Send) = &mut callback;
        self.active_callbacks
            .callbacks
            .lock()
            .unwrap()
            .push(unsafe { mem::transmute(callback) });

        loop {
            // So the loop does not get optimised out in --release
            thread::sleep(Duration::new(1u64, 0u32));
        }

        // Note: if we ever change this API so that `run` can return, then it is critical that
        // we remove the callback from `active_callbacks`.
    }

    #[inline]
    pub fn build_input_stream(
        &self,
        _device: &Device,
        _format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        unimplemented!();
    }

    #[inline]
    pub fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, CreationError>
    {
        let mut audio_unit = {
            let au_type = if cfg!(target_os = "ios") {
                // The DefaultOutput unit isn't available in iOS unfortunately.
                // RemoteIO is a sensible replacement.
                // See https://goo.gl/CWwRTx
                coreaudio::audio_unit::IOType::RemoteIO
            } else {
                coreaudio::audio_unit::IOType::DefaultOutput
            };

            AudioUnit::new(au_type)?
        };

        audio_unit.set_property(
            kAudioOutputUnitProperty_CurrentDevice,
            Scope::Global,
            Element::Output,
            Some(&device.audio_device_id),
        )?;

        // Set the stream in interleaved mode.
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
        audio_unit.set_property(
            kAudioUnitProperty_StreamFormat,
            Scope::Input,
            Element::Output,
            Some(&asbd)
        )?;

        // Determine the future ID of the stream.
        let mut streams_lock = self.streams.lock().unwrap();
        let stream_id = streams_lock
            .iter()
            .position(|n| n.is_none())
            .unwrap_or(streams_lock.len());

        // Register the callback that is being called by coreaudio whenever it needs data to be
        // fed to the audio buffer.
        let active_callbacks = self.active_callbacks.clone();
        audio_unit.set_render_callback(move |args: render_callback::Args<data::Raw>| unsafe {
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
                    let buffer = OutputBuffer { buffer: data_slice };
                    let unknown_type_buffer = UnknownTypeOutputBuffer::$SampleFormat(::OutputBuffer { target: Some(buffer) });
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
        {
            let inner = StreamInner {
                playing: true,
                audio_unit: audio_unit,
            };

            if stream_id == streams_lock.len() {
                streams_lock.push(Some(inner));
            } else {
                streams_lock[stream_id] = Some(inner);
            }
        }

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

pub struct InputBuffer<'a, T: 'a> {
    marker: ::std::marker::PhantomData<&'a T>,
}

pub struct OutputBuffer<'a, T: 'a> {
    buffer: &'a mut [T],
}

impl<'a, T> InputBuffer<'a, T> {
    #[inline]
    pub fn buffer(&self) -> &[T] {
        unimplemented!()
    }

    #[inline]
    pub fn finish(self) {
    }
}

impl<'a, T> OutputBuffer<'a, T>
    where T: Sample
{
    #[inline]
    pub fn buffer(&mut self) -> &mut [T] {
        &mut self.buffer
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn finish(self) {
        // Do nothing. We wrote directly to the buffer.
    }
}
