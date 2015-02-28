extern crate "coreaudio-rs" as coreaudio;
extern crate libc;

use self::coreaudio::audio_unit::{AudioUnit, Type, SubType};
use std::mem;
use std::sync::mpsc::{channel, Sender, Receiver};

type NumChannels = usize;

#[allow(dead_code)]
pub struct Voice {
    audio_unit: AudioUnit,
    ready_receiver: Receiver<()>,
    samples_sender: Sender<(Vec<f32>, NumChannels)>,
}

pub struct Buffer<'a, T: 'a> {
    samples_sender: Sender<(Vec<f32>, NumChannels)>,
    samples: Vec<T>,
    num_channels: NumChannels,
    marker: ::std::marker::PhantomData<&'a T>,
}

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

    // A channel for signalling that the audio unit is ready for data.
    let (ready_sender, ready_receiver) = channel();
    // A channel for sending the audio callback a pointer to the sample data.
    let (samples_sender, samples_receiver) = channel();

    let audio_unit_result = AudioUnit::new(Type::Output, SubType::HalOutput)
        .render_callback(box move |channels, num_frames| {

            let (samples, num_channels) = match samples_receiver.try_recv() {
                Ok((samples, num_channels)) => (samples, num_channels),
                _ => (vec![0.0; num_frames * channels.len()], channels.len()),
            };

            if let Err(_) = ready_sender.send(()) {
                return Err("Callback failed to send 'ready' message.".to_string());
            }

            assert!(num_frames == (samples.len() / num_channels) as usize,
                    "The number of input frames given differs from the number requested by the AudioUnit");

            for (i, frame) in samples.chunks(num_channels).enumerate() {
                for (channel, sample) in channels.iter_mut().zip(frame.iter()) {
                    channel[i] = *sample;
                }
            }

            Ok(())

        })
        .start();

    match audio_unit_result {
        Ok(audio_unit) => Ok(Voice {
            audio_unit: audio_unit,
            ready_receiver: ready_receiver,
            samples_sender: samples_sender
        }),
        Err(err) => {
            use ::std::error::Error;
            Err(err.description().to_string())
        },
    }

}

