extern crate cpal;
extern crate futures;

use futures::stream::Stream;
use futures::task;
use futures::task::Executor;
use futures::task::Run;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

struct MyExecutor;

impl Executor for MyExecutor {
    fn execute(&self, r: Run) {
        r.run();
    }
}

fn main() {
    let endpoint = cpal::default_endpoint().expect("Failed to get default endpoint");
    let format = endpoint.supported_formats().unwrap().next().expect("Failed to get endpoint format");

    let event_loop = cpal::EventLoop::new();
    let executor = Arc::new(MyExecutor);

    let (mut voice, stream) = cpal::Voice::new(&endpoint, &format, &event_loop).expect("Failed to create a voice");

    // Produce a sinusoid of maximum amplitude.
    let samples_rate = format.samples_rate.0 as f32;
    let mut data_source = (0u64..).map(move |t| t as f32 * 440.0 * 2.0 * 3.141592 / samples_rate)     // 440 Hz
                                  .map(move |t| t.sin());

    voice.play();
    task::spawn(stream.for_each(move |buffer| -> Result<_, ()> {
        match buffer {
            cpal::UnknownTypeBuffer::U16(mut buffer) => {
                for (sample, value) in buffer.chunks_mut(format.channels.len()).zip(&mut data_source) {
                    let value = ((value * 0.5 + 0.5) * std::u16::MAX as f32) as u16;
                    for out in sample.iter_mut() { *out = value; }
                }
            },

            cpal::UnknownTypeBuffer::I16(mut buffer) => {
                for (sample, value) in buffer.chunks_mut(format.channels.len()).zip(&mut data_source) {
                    let value = (value * std::i16::MAX as f32) as i16;
                    for out in sample.iter_mut() { *out = value; }
                }
            },

            cpal::UnknownTypeBuffer::F32(mut buffer) => {
                for (sample, value) in buffer.chunks_mut(format.channels.len()).zip(&mut data_source) {
                    for out in sample.iter_mut() { *out = value; }
                }
            },
        };

        Ok(())
    })).execute(executor);

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(500));
            voice.pause();
            thread::sleep(Duration::from_millis(500));
            voice.play();
        }
    });

    event_loop.run();
}
