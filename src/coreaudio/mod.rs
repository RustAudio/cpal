extern crate coreaudio_rs as coreaudio;
extern crate libc;

use std::sync::mpsc::{channel, Sender, Receiver};
use std::mem;

use CreationError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SamplesRate;
use ChannelPosition;

mod enumerate;

pub use self::enumerate::{EndpointsIterator,
                          SupportedFormatsIterator,
                          get_default_endpoint};

use self::coreaudio::audio_unit::{AudioUnit, Type, SubType};

#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        Ok(vec!(Format {
            channels: vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
            samples_rate: SamplesRate(64),
            data_type: SampleFormat::F32
        }).into_iter())
    }

    pub fn get_name(&self) -> String {
        "Default AudioUnit Endpoint".to_string()
    }
}

pub struct Buffer<'a, T: 'a> {
    samples_sender: Sender<(Vec<f32>, NumChannels)>,
    samples: Vec<T>,
    num_channels: NumChannels,
    len: usize,
    marker: ::std::marker::PhantomData<&'a T>,
}

impl<'a, T> Buffer<'a, T> {
    #[inline]
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        &mut self.samples[..]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
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

type NumChannels = usize;
type NumFrames = usize;

pub struct Voice {
    audio_unit: AudioUnit,
    ready_receiver: Receiver<(NumChannels, NumFrames)>,
    samples_sender: Sender<(Vec<f32>, NumChannels)>,
}

unsafe impl Sync for Voice {}
unsafe impl Send for Voice {}

impl Voice {
    pub fn new(endpoint: &Endpoint, format: &Format) -> Result<Voice, CreationError> {
        // A channel for signalling that the audio unit is ready for data.
        let (ready_sender, ready_receiver) = channel();
        // A channel for sending the audio callback a pointer to the sample data.
        let (samples_sender, samples_receiver) = channel();

        let audio_unit_result = AudioUnit::new(Type::Output, SubType::HalOutput)
            .render_callback(Box::new(move |channels, num_frames| {
                if let Err(_) = ready_sender.send((channels.len(), num_frames)) {
                    return Err("Callback failed to send 'ready' message.".to_string());
                }
                loop {
                    if let Ok((samples, num_channels)) = samples_receiver.try_recv() {
                        let samples: Vec<f32> = samples;
                        assert!(num_frames == (samples.len() / num_channels) as usize,
                                "The number of input frames given differs from the number \
                                requested by the AudioUnit: {:?} and {:?} respectively",
                                (samples.len() / num_channels as usize), num_frames);
                        for (i, frame) in samples.chunks(num_channels).enumerate() {
                            for (channel, sample) in channels.iter_mut().zip(frame.iter()) {
                                channel[i] = *sample;
                            }
                        }
                        break;
                    };
                }
                Ok(())

            }))
            .start();

        match audio_unit_result {
            Ok(audio_unit) => Ok(Voice {
                audio_unit: audio_unit,
                ready_receiver: ready_receiver,
                samples_sender: samples_sender
            }),
            Err(_) => {
                Err(CreationError::DeviceNotAvailable)
            },
        }
    }

    pub fn append_data<'a, T>(&'a mut self, max_elements: usize) -> Buffer<'a, T> where T: Clone {
        // Block until the audio callback is ready for more data.
        loop {
            if let Ok((channels, frames)) = self.ready_receiver.try_recv() {
                let buffer_size = ::std::cmp::min(channels * frames, max_elements);
                return Buffer {
                    samples_sender: self.samples_sender.clone(),
                    samples: vec![unsafe{ mem::uninitialized() }; buffer_size],
                    num_channels: channels as usize,
                    marker: ::std::marker::PhantomData,
                    len: 64
                }
            }
        }
    }

    #[inline]
    pub fn play(&mut self) {
        // implicitly playing
    }

    #[inline]
    pub fn pause(&mut self) {
        unimplemented!()
    }

    pub fn get_pending_samples(&self) -> usize {
        0
    }

    pub fn underflowed(&self) -> bool {
        false
    }
}
