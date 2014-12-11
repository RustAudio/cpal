extern crate cpal;

fn main() {
    let mut channel = cpal::Channel::new();

    assert!(channel.get_samples_format() == cpal::SampleFormat::U16);

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
        let mut buffer = channel.append_data();

        for sample in buffer.chunks_mut(4) {
            let value = data_source.next().unwrap();

            let mut writer = std::io::BufWriter::new(sample);
            writer.write_le_u16(value).unwrap();
            writer.write_le_u16(value).unwrap();
        }
    }
}
