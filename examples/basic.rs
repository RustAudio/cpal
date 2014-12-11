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
            })
            .map(|v| (v, v));

    loop {
        let mut buffer = channel.append_data::<(u16, u16)>();

        for value in buffer.iter_mut() {
            *value = data_source.next().unwrap();
        }
    }
}
