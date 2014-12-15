extern crate cpal;

fn main() {
    let mut channel = cpal::Channel::new();

    // producing a sinusoid
    let mut data_source =
        std::iter::iterate(0.0f32, |f| f + 0.03)
            .map(|angle| {
                use std::num::FloatMath;
                use std::num::Int;

                let angle = angle.sin();

                let max: u16 = Int::max_value();
                let value = (max as f32 / 2.0) + (angle * (max as f32 / 2.0));
                value as u16
            });

    loop {
        let mut buffer = channel.append_data(1, cpal::SamplesRate(44100));

        for sample in buffer.samples() {
            let value = data_source.next().unwrap();
            *sample = value;
        }
    }
}
