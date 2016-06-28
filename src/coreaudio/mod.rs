extern crate coreaudio_rs as coreaudio;
extern crate libc;

use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::RefCell;
use std::mem;
use std::cmp;
use std::marker::PhantomData;

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

use self::coreaudio::audio_unit::{AudioUnit, IOType};

#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint;

impl Endpoint {
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        Ok(vec!(Format {
            channels: vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
            samples_rate: SamplesRate(44100),
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
    marker: PhantomData<&'a T>,
    pending_samples: Arc<AtomicUsize>
}

impl<'a, T> Buffer<'a, T> {
    #[inline]
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        &mut self.samples[..]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    #[inline]
    pub fn finish(self) {
        let Buffer { samples_sender, samples, num_channels, pending_samples, .. } = self;
        // TODO: At the moment this assumes the Vec<T> is a Vec<f32>.
        // Need to add T: Sample and use Sample::to_vec_f32.
        let num_samples = samples.len();
        let samples = unsafe { mem::transmute(samples) };
        pending_samples.fetch_add(num_samples, Ordering::SeqCst);
        match samples_sender.send((samples, num_channels)) {
            Err(_) => panic!("Failed to send samples to audio unit callback."),
            Ok(()) => (),
        }
    }
}

type NumChannels = usize;
type NumFrames = usize;

#[allow(dead_code)] // the audio_unit will be dropped if we don't hold it.
pub struct Voice {
    audio_unit: AudioUnit,
    ready_receiver: Receiver<(NumChannels, NumFrames)>,
    samples_sender: Sender<(Vec<f32>, NumChannels)>,
    underflow: Arc<Mutex<RefCell<bool>>>,
    last_ready: Arc<Mutex<RefCell<Option<(NumChannels, NumFrames)>>>>,
    pending_samples: Arc<AtomicUsize>
}

unsafe impl Sync for Voice {}
unsafe impl Send for Voice {}

impl Voice {
    pub fn new(_: &Endpoint, _: &Format) -> Result<Voice, CreationError> {
        // A channel for signalling that the audio unit is ready for data.
        let (ready_sender, ready_receiver) = channel();
        // A channel for sending the audio callback a pointer to the sample data.
        let (samples_sender, samples_receiver) = channel();

        let underflow = Arc::new(Mutex::new(RefCell::new(false)));
        let uf_clone = underflow.clone();

        let pending_samples: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let pending_samples_c = pending_samples.clone();

        let audio_unit_result = AudioUnit::new(IOType::HalOutput);

        if let Ok(mut audio_unit) = audio_unit_result {
            if let Ok(()) = audio_unit.set_render_callback(Some(Box::new(move |channels: &mut[&mut[f32]], num_frames: NumFrames| {
                if let Err(_) = ready_sender.send((channels.len(), num_frames)) {
                    return Err("Callback failed to send 'ready' message.".to_string());
                }
                loop {
                    if let Ok((samples, num_channels)) = samples_receiver.try_recv() {
                        let samples: Vec<f32> = samples;
                        if let Ok(uf) = uf_clone.lock() {
                            *(uf.borrow_mut()) = num_frames > samples.len() / num_channels;
                        } else { return Err("Couldn't lock underflow flag field.".to_string()) }

                        pending_samples_c.fetch_sub(samples.len(), Ordering::SeqCst);

                        for (i, frame) in samples.chunks(num_channels).enumerate() {
                            for (channel, sample) in channels.iter_mut().zip(frame.iter()) {
                                channel[i] = *sample;
                            }
                        }

                        break;
                    };
                }
                Ok(())

            }))) {
                if let Ok(()) = audio_unit.start() {
                    return Ok(Voice {
                        audio_unit: audio_unit,
                        ready_receiver: ready_receiver,
                        samples_sender: samples_sender,
                        underflow: underflow,
                        last_ready: Arc::new(Mutex::new(RefCell::new(None))),
                        pending_samples: pending_samples
                    })
                }
            }
        }

        Err(CreationError::DeviceNotAvailable)
    }

    pub fn append_data<'a, T>(&'a mut self, max_elements: usize) -> Buffer<'a, T> where T: Clone {
        // Block until the audio callback is ready for more data.
        let (channels, frames) = self.block_until_ready();
        let buffer_size = cmp::min(channels * frames, max_elements);
        Buffer {
            samples_sender: self.samples_sender.clone(),
            samples: vec![unsafe { mem::uninitialized() }; buffer_size],
            num_channels: channels as usize,
            marker: PhantomData,
            pending_samples: self.pending_samples.clone()
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

    #[inline]
    pub fn get_period(&self) -> usize {
        if let Some(ready) = self.update_last_ready() {
            (ready.0 * ready.1) as usize
        } else {
            0
        }
    }

    #[inline]
    pub fn get_pending_samples(&self) -> usize {
        self.pending_samples.load(Ordering::Relaxed)
    }

    /// Attempts to store the most recent ready message into the internal
    /// ref cell, then return the last ready message. If the last ready hasn't
    /// been reset with `clear_last_ready`, then it will not be set and the
    /// current value will be returned. Else, the ready_receiver will be
    /// try_recv'd and if it is ready, the last ready will be set and returned.
    /// Finally, if the ready_receiver had no data at try_recv, None will be
    /// returned.
    #[inline]
    fn update_last_ready(&self) -> Option<(NumChannels, NumFrames)> {
        let refcell = self.last_ready.lock().unwrap();
        let data = refcell.borrow();
        if let Some(s) = *data {
            //
            return Some(s);
        } else {
            drop(data);
            let mut data = refcell.borrow_mut();
            if let Ok(ready) = self.ready_receiver.try_recv() {
                // the audiounit is ready so we can set last_ready
                *data = Some(ready);
                return *data;
            }
        }
        None
    }

    /// Block until ready to send data. This checks last_ready first. In any
    /// case, last_ready will be set to None when this function returns.
    fn block_until_ready(&self) -> (NumChannels, NumFrames) {
        let refcell = self.last_ready.lock().unwrap();
        let data = refcell.borrow();
        if let Some(s) = *data {
            drop(data);
            let mut data = refcell.borrow_mut();
            *data = None;
            return s;
        } else {
            match self.ready_receiver.recv() {
                Ok(ready) => {
                    return ready;
                },
                Err(e) => panic!("Couldn't receive a ready message: \
                                  {:?}", e)
            }
        }
    }

    #[inline]
    pub fn underflowed(&self) -> bool {
        let uf = self.underflow.lock().unwrap();
        let v = uf.borrow();
        *v
    }
}
