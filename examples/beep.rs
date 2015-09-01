extern crate cpal;

fn main() {
    let mut channel = cpal::Voice::new(&cpal::get_default_endpoint().unwrap()).unwrap();

    // Produce a sinusoid of maximum amplitude.
    let mut data_source = (0u64..).map(|t| t as f32 * 0.03)
                                  .map(|t| t.sin());

    loop {
        match channel.append_data(32768) {
            cpal::UnknownTypeBuffer::U16(mut buffer) => {
                for (sample, value) in buffer.chunks_mut(2).zip(&mut data_source) {
                    let value = ((value * 0.5 + 0.5) * std::u16::MAX as f32) as u16;
                    sample[0] = value;
                    sample[1] = value;
                }
            },

            cpal::UnknownTypeBuffer::I16(mut buffer) => {
                for (sample, value) in buffer.chunks_mut(2).zip(&mut data_source) {
                    let value = (value * std::i16::MAX as f32) as i16;
                    sample[0] = value;
                    sample[1] = value;
                }
            },

            cpal::UnknownTypeBuffer::F32(mut buffer) => {
                for (sample, value) in buffer.chunks_mut(2).zip(&mut data_source) {
                    sample[0] = value;
                    sample[1] = value;
                }
            },
        }

        channel.play();
    }
}
