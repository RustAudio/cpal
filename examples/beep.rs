extern crate cpal;

// TODO: manual replacement for unstable `std::iter::iterate`
struct Iter {
    value: f32,
}

impl Iterator for Iter {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.value += 0.03;
        Some(self.value)
    }
}

fn main() {
    let mut channel = cpal::Voice::new();

    // producing a sinusoid
    let mut data_source = Iter { value: 0.0 }
            .map(|angle| {
                let angle = angle.sin();

                let max: u16 = std::u16::MAX;
                let value = (max as f32 / 2.0) + (angle * (max as f32 / 2.0));
                value as u16
            });

    loop {
        {
            let mut buffer = channel.append_data(1, cpal::SamplesRate(44100), 32768);

            for sample in buffer.iter_mut() {
                let value = data_source.next().unwrap();
                *sample = value;
            }
        }

        channel.play();
    }
}
