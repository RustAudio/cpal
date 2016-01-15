extern crate cpal;

fn main() {
    let endpoint = cpal::get_default_endpoint().expect("Failed to get default endpoint");
    let format = endpoint.get_supported_formats_list().unwrap().next().expect("Failed to get endpoint format");
    let mut channel = cpal::Voice::new(&endpoint, &format).expect("Failed to create a channel");

    // Produce a sinusoid of maximum amplitude.
    let mut data_source = (0u64..).map(|t| t as f32 * 440.0 * 2.0 * 3.141592 / format.samples_rate.0 as f32)     // 440 Hz
                                  .map(|t| t.sin());

    loop {
        match channel.append_data(32768) {
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
        }

        channel.play();
    }
}
