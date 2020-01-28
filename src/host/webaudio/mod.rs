extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

use self::js_sys::eval;
use self::wasm_bindgen::prelude::*;
use self::wasm_bindgen::JsCast;
use self::web_sys::{AudioContext, AudioContextOptions};
use crate::{
    BuildStreamError, Data, DefaultFormatError, DeviceNameError, DevicesError, Format,
    PauseStreamError, PlayStreamError, StreamError, SupportedFormat, SupportedFormatsError,
};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex, RwLock};
use traits::{DeviceTrait, HostTrait, StreamTrait};
use {BackendSpecificError, SampleFormat};

/// Content is false if the iterator is empty.
pub struct Devices(bool);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device;

pub struct Host;

pub struct Stream {
    ctx: Arc<AudioContext>,
    on_ended_closures: Vec<Arc<RwLock<Option<Closure<dyn FnMut()>>>>>,
}

pub type SupportedInputFormats = ::std::vec::IntoIter<SupportedFormat>;
pub type SupportedOutputFormats = ::std::vec::IntoIter<SupportedFormat>;

impl Host {
    pub fn new() -> Result<Self, crate::HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;

    fn is_available() -> bool {
        // Assume this host is always available on webaudio.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }
}

impl Devices {
    fn new() -> Result<Self, DevicesError> {
        Ok(Self::default())
    }
}

impl Device {
    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok("Default Device".to_owned())
    }

    #[inline]
    fn supported_input_formats(&self) -> Result<SupportedInputFormats, SupportedFormatsError> {
        unimplemented!();
    }

    #[inline]
    fn supported_output_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        // TODO: right now cpal's API doesn't allow flexibility here
        //       "44100" and "2" (channels) have also been hard-coded in the rest of the code ; if
        //       this ever becomes more flexible, don't forget to change that
        //       According to https://developer.mozilla.org/en-US/docs/Web/API/BaseAudioContext/createBuffer
        //       browsers must support 1 to 32 channels at leats and 8,000 Hz to 96,000 Hz.
        //
        //       UPDATE: We can do this now. Might be best to use `crate::COMMON_SAMPLE_RATES` and
        //       filter out those that lay outside the range specified above.
        Ok(vec![SupportedFormat {
            channels: 2,
            min_sample_rate: ::SampleRate(44100),
            max_sample_rate: ::SampleRate(44100),
            data_type: ::SampleFormat::F32,
        }]
        .into_iter())
    }

    #[inline]
    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        unimplemented!();
    }

    #[inline]
    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        // TODO: because it is hard coded, see supported_output_formats.
        Ok(Format {
            channels: 2,
            sample_rate: ::SampleRate(44100),
            data_type: ::SampleFormat::F32,
        })
    }
}

impl DeviceTrait for Device {
    type SupportedInputFormats = SupportedInputFormats;
    type SupportedOutputFormats = SupportedOutputFormats;
    type Stream = Stream;

    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    #[inline]
    fn supported_input_formats(
        &self,
    ) -> Result<Self::SupportedInputFormats, SupportedFormatsError> {
        Device::supported_input_formats(self)
    }

    #[inline]
    fn supported_output_formats(
        &self,
    ) -> Result<Self::SupportedOutputFormats, SupportedFormatsError> {
        Device::supported_output_formats(self)
    }

    #[inline]
    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_input_format(self)
    }

    #[inline]
    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_output_format(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        _format: &Format,
        _data_callback: D,
        _error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        unimplemented!()
    }

    /// Create an output stream.
    fn build_output_stream_raw<D, E>(
        &self,
        format: &Format,
        data_callback: D,
        _error_callback: E,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        assert_eq!(
            format.data_type,
            SampleFormat::F32,
            "WebAudio backend currently only supports `f32` data",
        );

        // Use a buffer period of 1/3s for this early proof of concept.
        let buffer_length = (format.sample_rate.0 as f64 / 3.0).round() as usize;
        let data_callback = Arc::new(Mutex::new(Box::new(data_callback)));

        // Create the WebAudio stream.
        let mut stream_opts = AudioContextOptions::new();
        stream_opts.sample_rate(format.sample_rate.0 as f32);
        let ctx = Arc::new(
            AudioContext::new_with_context_options(&stream_opts).map_err(
                |err| -> BuildStreamError {
                    let description = format!("{:?}", err);
                    let err = BackendSpecificError { description };
                    err.into()
                },
            )?,
        );

        // A container for managing the lifecycle of the audio callbacks.
        let mut on_ended_closures: Vec<Arc<RwLock<Option<Closure<dyn FnMut()>>>>> = Vec::new();

        // A cursor keeping track of the current time at which new frames should be scheduled.
        let time = Arc::new(RwLock::new(0f64));

        // Create a set of closures / callbacks which will continuously fetch and schedule sample playback.
        // Starting with two workers, eg a front and back buffer so that audio frames can be fetched in the background.
        for _i in 0..2 {
            let format = format.clone();
            let data_callback_handle = data_callback.clone();
            let ctx_handle = ctx.clone();
            let time_handle = time.clone();

            // A set of temporary buffers to be used for intermediate sample transformation steps.
            let mut temporary_buffer = vec![0f32; buffer_length * format.channels as usize];
            let mut temporary_channel_buffer = vec![0f32; buffer_length];

            // Create a webaudio buffer which will be reused to avoid allocations.
            let ctx_buffer = ctx
                .create_buffer(
                    format.channels as u32,
                    buffer_length as u32,
                    format.sample_rate.0 as f32,
                )
                .map_err(|err| -> BuildStreamError {
                    let description = format!("{:?}", err);
                    let err = BackendSpecificError { description };
                    err.into()
                })?;

            // A self reference to this closure for passing to future audio event calls.
            let on_ended_closure: Arc<RwLock<Option<Closure<dyn FnMut()>>>> =
                Arc::new(RwLock::new(None));
            let on_ended_closure_handle = on_ended_closure.clone();

            on_ended_closure
                .write()
                .unwrap()
                .replace(Closure::wrap(Box::new(move || {
                    let time_at_start_of_buffer = {
                        let time_at_start_of_buffer = time_handle
                            .read()
                            .expect("Unable to get a read lock on the time cursor");
                        // Synchronise first buffer as necessary (eg. keep the time value referenced to the context clock).
                        if *time_at_start_of_buffer > 0.001 {
                            *time_at_start_of_buffer
                        } else {
                            // 25ms of time to fetch the first sample data, increase to avoid initial underruns.
                            ctx_handle.current_time() + 0.025
                        }
                    };

                    // Populate the sample data into an interleaved temporary buffer.
                    {
                        let len = temporary_buffer.len();
                        let data = temporary_buffer.as_mut_ptr() as *mut ();
                        let sample_format = SampleFormat::F32;
                        let mut data = unsafe { Data::from_parts(data, len, sample_format) };
                        let mut data_callback = data_callback_handle.lock().unwrap();
                        (data_callback.deref_mut())(&mut data);
                    }

                    // Deinterleave the sample data and copy into the audio context buffer.
                    // We do not reference the audio context buffer directly eg getChannelData.
                    // As wasm-bindgen only gives us a copy, not a direct reference.
                    for channel in 0..(format.channels as usize) {
                        for i in 0..buffer_length {
                            temporary_channel_buffer[i] =
                                temporary_buffer[(format.channels as usize) * i + channel];
                        }
                        ctx_buffer
                            .copy_to_channel(&mut temporary_channel_buffer, channel as i32)
                            .expect("Unable to write sample data into the audio context buffer");
                    }

                    // Create an AudioBufferSourceNode, scheduled it to playback the reused buffer in the future.
                    let source = ctx_handle
                        .create_buffer_source()
                        .expect("Unable to create a webaudio buffer source");
                    source.set_buffer(Some(&ctx_buffer));
                    source
                        .connect_with_audio_node(&ctx_handle.destination())
                        .expect(
                        "Unable to connect the web audio buffer source to the context destination",
                    );
                    source.set_onended(Some(
                        on_ended_closure_handle
                            .read()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .as_ref()
                            .unchecked_ref(),
                    ));

                    source
                        .start_with_when(time_at_start_of_buffer)
                        .expect("Unable to start the webaudio buffer source");

                    // Keep track of when the next buffer worth of samples should be played.
                    *time_handle.write().unwrap() = time_at_start_of_buffer
                        + (buffer_length as f64 / format.sample_rate.0 as f64);
                }) as Box<dyn FnMut()>));

            on_ended_closures.push(on_ended_closure);
        }

        Ok(Stream {
            ctx,
            on_ended_closures,
        })
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        let window = web_sys::window().unwrap();
        match self.ctx.resume() {
            Ok(_) => {
                // Begin webaudio playback, initially scheduling the closures to fire on a timeout event.
                let mut offset_ms = 10;
                for on_ended_closure in self.on_ended_closures.iter() {
                    window
                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                            on_ended_closure
                                .read()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .as_ref()
                                .unchecked_ref(),
                            offset_ms,
                        )
                        .unwrap();
                    offset_ms += 333 / 2;
                }
                Ok(())
            }
            Err(err) => {
                let description = format!("{:?}", err);
                let err = BackendSpecificError { description };
                Err(err.into())
            }
        }
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        match self.ctx.suspend() {
            Ok(_) => Ok(()),
            Err(err) => {
                let description = format!("{:?}", err);
                let err = BackendSpecificError { description };
                Err(err.into())
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.ctx.close();
    }
}

impl Default for Devices {
    fn default() -> Devices {
        // We produce an empty iterator if the WebAudio API isn't available.
        Devices(is_webaudio_available())
    }
}

impl Iterator for Devices {
    type Item = Device;
    #[inline]
    fn next(&mut self) -> Option<Device> {
        if self.0 {
            self.0 = false;
            Some(Device)
        } else {
            None
        }
    }
}

#[inline]
fn default_input_device() -> Option<Device> {
    unimplemented!();
}

#[inline]
fn default_output_device() -> Option<Device> {
    if is_webaudio_available() {
        Some(Device)
    } else {
        None
    }
}

// Detects whether the `AudioContext` global variable is available.
fn is_webaudio_available() -> bool {
    if let Ok(audio_context_is_defined) = eval("typeof AudioContext !== 'undefined'") {
        audio_context_is_defined.as_bool().unwrap()
    } else {
        false
    }
}
