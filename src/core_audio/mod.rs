extern crate "core_audio-sys" as core_audio;
extern crate libc;

use self::core_audio::audio_unit as au;
use std::mem;
use std::ptr::null_mut;
use std::sync::mpsc::{channel, Sender, Receiver};

type NumChannels = usize;

#[allow(dead_code)]
pub struct Voice {
    audio_unit: *mut au::AudioUnit,
    ready_receiver: Receiver<()>,
    samples_sender: Sender<(Vec<f32>, NumChannels)>,
}

pub struct Buffer<'a, T: 'a> {
    samples_sender: Sender<(Vec<f32>, NumChannels)>,
    samples: Vec<T>,
    num_channels: NumChannels,
    marker: ::std::marker::PhantomData<&'a T>,
}

struct CallbackConnection {
    ready_sender: Sender<()>,
    samples_receiver: Receiver<(Vec<f32>, NumChannels)>,
}


const INPUT_SCOPE    : au::AudioUnitScope   = au::kAudioUnitScope_Input;
const OUTPUT_ELEMENT : au::AudioUnitElement = 0;


impl Voice {

    pub fn new() -> Voice {
        new_voice().unwrap()
    }

    pub fn get_channels(&self) -> ::ChannelsCount {
        // TODO: use AudioUnitGetProperty...
        2
    }

    pub fn get_samples_rate(&self) -> ::SamplesRate {
        // TODO: use AudioUnitGetProperty...
        ::SamplesRate(44100)
    }

    pub fn get_samples_format(&self) -> ::SampleFormat {
        // TODO: use AudioUnitGetProperty...
        ::SampleFormat::F32
    }

    pub fn append_data<'a, T>(&'a mut self, buffer_size: usize) -> Buffer<'a, T> where T: Clone {
        while let None = self.ready_receiver.try_recv().ok() {}
        Buffer {
            samples_sender: self.samples_sender.clone(),
            samples: vec![unsafe{ mem::uninitialized() }; buffer_size],
            num_channels: 2,
            marker: ::std::marker::PhantomData,
        }
    }

    pub fn play(&mut self) {
        // TODO
    }

    pub fn pause(&mut self) {
        // TODO
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        unsafe {
            check_errors(au::AudioOutputUnitStop(*self.audio_unit)).unwrap();
            check_errors(au::AudioUnitUninitialize(*self.audio_unit)).unwrap();
        }
    }
}

impl<'a, T> Buffer<'a, T> {
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        &mut self.samples[..]
    }
    pub fn finish(self) {
        let Buffer { samples_sender, samples, num_channels, .. } = self;
        // TODO: At the moment this assumes the Vec<T> is a Vec<f32>.
        // Need to add T: Sample and use Sample::to_vec_f32.
        let samples = unsafe { mem::transmute(samples) };
        match samples_sender.send((samples, num_channels)) {
            Err(_) => panic!("Failed to send samples to audio unit callback."),
            Ok(()) => (),
        }
    }
}


/// Construct a new Voice.
fn new_voice() -> Result<Voice, String> {

    let mut audio_unit = try!(default_audio_unit());

    // A channel for signalling that the audio unit is ready for data.
    let (ready_sender, ready_receiver) = channel();
    // A channel for sending the audio callback a pointer to the sample data.
    let (samples_sender, samples_receiver) = channel();

    let callback_connection = box CallbackConnection {
        ready_sender: ready_sender,
        samples_receiver: samples_receiver,
    };

    let size_of_render_callback_struct = mem::size_of::<au::AURenderCallbackStruct>() as u32;

    unsafe {
        // Setup render callback.
        let render_callback = au::AURenderCallbackStruct {
            inputProc: Some(input_proc), // TODO
            inputProcRefCon: mem::transmute(callback_connection),
        };

        try!(check_errors(au::AudioUnitSetProperty(audio_unit,
                                                   au::kAudioUnitProperty_SetRenderCallback,
                                                   INPUT_SCOPE,
                                                   OUTPUT_ELEMENT,
                                                   &render_callback as *const _ as *const libc::c_void,
                                                   size_of_render_callback_struct)));

        // Initialise the audio unit!
        try!(check_errors(au::AudioUnitInitialize(audio_unit)));
        try!(check_errors(au::AudioOutputUnitStart(audio_unit)));

        Ok(Voice {
            audio_unit: &mut audio_unit as *mut au::AudioUnit,
            ready_receiver: ready_receiver,
            samples_sender: samples_sender,
        })
    }
}


/// Construct and initiate a new, default AudioUnit.
fn default_audio_unit() -> Result<au::AudioUnit, String> {

    // A description of the audio unit we desire.
    let desc = au::AudioComponentDescription {
        componentType         : au::kAudioUnitType_Output,
        componentSubType      : au::kAudioUnitSubType_HALOutput,
        componentManufacturer : au::kAudioUnitManufacturer_Apple,
        componentFlags        : 0,
        componentFlagsMask    : 0,
    };

    unsafe {
        // Find the default audio unit for the description.
        let component = match au::AudioComponentFindNext(null_mut(), &desc as *const _) {
            component if component == null_mut() => panic!("Could not find a default audio device."),
            component                            => component,
        };

        // Get an instance of the default audio unit using the component.
        let mut audio_unit: au::AudioUnit = mem::uninitialized();
        au::AudioComponentInstanceNew(component, &mut audio_unit as *mut au::AudioUnit);

        Ok(audio_unit as au::AudioUnit)
    }
}


/// Callback procedure that will be called each time our audio_unit requests audio.
extern "C" fn input_proc(in_ref_con: *mut libc::c_void,
                         _io_action_flags: *mut au::AudioUnitRenderActionFlags,
                         _in_time_stamp: *const au::AudioTimeStamp,
                         _in_bus_number: au::UInt32,
                         in_number_frames: au::UInt32,
                         io_data: *mut au::AudioBufferList) -> au::OSStatus {
    let callback_connection = in_ref_con as *mut CallbackConnection;
    let (samples, num_channels) = match unsafe { (*callback_connection).samples_receiver.try_recv() } {
        Ok((samples, num_channels)) => (samples, num_channels),
        _ => (vec![0.0; 1024], 2),
    };

    if let Err(_) = unsafe { (*callback_connection).ready_sender.send(()) } {
        return -1500
    }

    assert!(in_number_frames == (samples.len() / num_channels) as u32,
            "The number of input frames given differs from the number requested by the AudioUnit");

    let mut channels: Vec<&mut [f32]> = unsafe {
        (0..num_channels)
            .map(|i| {
                let slice_ptr = (*io_data).mBuffers[i].mData as *mut libc::c_float;
                ::std::slice::from_raw_parts_mut(slice_ptr, in_number_frames as usize)
            })
            .collect()
    };

    for (i, frame) in samples.chunks(num_channels).enumerate() {
        for (channel, sample) in channels.iter_mut().zip(frame.iter()) {
            channel[i] = *sample;
        }
    }

    0
}


/// Convert the AudioUnit OSStatus result into a Rust Result.
fn check_errors(result: au::OSStatus) -> Result<(), String> {
    if result == 0 { return Ok(()); }
    Err(format!("OSStatus {:?}: {:?}", result, match result {
        -1500  => "An unspecified error has occurred",
        -1501  => "System sound client message timed out",
        -10847 => "Unauthorized",
        -10848 => "Invalid offline render",
        -10849 => "Initialized",
        -10850 => "Property not in use",
        -10851 => "Invalid property type",
        -10863 => "Cannot do in current context",
        -10865 => "Property not writeable",
        -10866 => "Invalid scope",
        -10867 => "Uninitialized",
        -10868 => "Format not supported",
        -10869 => "File not specified",
        -10870 => "Unknown file type",
        -10871 => "Invalid file",
        -10872 => "Instrument type not found",
        -10873 => "Illegal instrument",
        -10874 => "Too many frames to process",
        -10875 => "Failed initialization",
        -10876 => "No connection",
        -10877 => "Invalid element",
        -10878 => "Invalid parameter",
        -10879 => "Invalid property",
        _      => "Unknown error",
    }))
}

