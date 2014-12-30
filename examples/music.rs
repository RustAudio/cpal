extern crate cpal;
extern crate vorbis;

use std::io::BufReader;

fn main() {
    let mut channel = cpal::Voice::new();
    channel.play();

    let mut decoder = vorbis::Decoder::new(BufReader::new(include_bytes!("music.ogg")))
        .unwrap();

    'main: for packet in decoder.packets() {
        let packet = packet.unwrap();
        let vorbis::Packet { channels, rate, data, .. } = packet;

        let mut data = data.as_slice();

        loop {
            if data.len() == 0 {
                continue 'main;
            }

            {
                let mut buffer = channel.append_data(channels, cpal::SamplesRate(rate as u32), 
                                                     data.len());
                let mut buffer = buffer.iter_mut();

                loop {
                    let next_sample = match data.get(0) {
                        Some(s) => *s,
                        None => continue 'main
                    };

                    if let Some(output) = buffer.next() {
                        *output = next_sample as u16;
                        data = data.slice_from(1);
                    } else {
                        break;
                    }
                }
            }

            channel.play();
        }
    }
}
