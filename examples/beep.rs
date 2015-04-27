extern crate cpal;

fn main() {
    let mut channel = cpal::Voice::new();

    // Produce a sinusoid of maximum amplitude.
    let max = std::u16::MAX as f32;
    let mut data_source = (0u64..).map(|t| t as f32 * 0.03)
                                  .map(|t| ((t.sin() * 0.5 + 0.5) * max) as u16);

    loop {
        {
            let mut buffer = channel.append_data(1, cpal::SamplesRate(44100), 32768);

            for (sample, value) in buffer.iter_mut().zip(&mut data_source) {
                *sample = value;
            }
        }

        channel.play();
    }
}
