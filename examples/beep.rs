extern crate cpal;

fn main() {
    let endpoint = cpal::default_endpoint().expect("Failed to get default endpoint");
    let format = endpoint
        .supported_formats()
        .unwrap()
        .next()
        .expect("Failed to get endpoint format");

    let event_loop = cpal::EventLoop::new();
    let voice_id = event_loop.build_voice(&endpoint, &format).unwrap();
    event_loop.play(voice_id);

    let samples_rate = format.samples_rate.0 as f32;
    let mut sample_clock = 0f32;

    // Produce a sinusoid of maximum amplitude.
    let mut next_value = || {
        sample_clock = (sample_clock + 1.0) % samples_rate;
        (sample_clock * 440.0 * 2.0 * 3.141592 / samples_rate).sin()
    };

    event_loop.run(move |_, buffer| {
        match buffer {
            cpal::UnknownTypeBuffer::U16(mut buffer) => {
                for sample in buffer.chunks_mut(format.channels.len()) {
                    let value = ((next_value() * 0.5 + 0.5) * std::u16::MAX as f32) as u16;
                    for out in sample.iter_mut() {
                        *out = value;
                    }
                }
            },

            cpal::UnknownTypeBuffer::I16(mut buffer) => {
                for sample in buffer.chunks_mut(format.channels.len()) {
                    let value = (next_value() * std::i16::MAX as f32) as i16;
                    for out in sample.iter_mut() {
                        *out = value;
                    }
                }
            },

            cpal::UnknownTypeBuffer::F32(mut buffer) => {
                for sample in buffer.chunks_mut(format.channels.len()) {
                    let value = next_value();
                    for out in sample.iter_mut() {
                        *out = value;
                    }
                }
            },
        };
    });
}
