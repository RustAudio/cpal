extern crate cpal;
extern crate vorbis;

use std::io::BufReader;

fn main() {
    let mut channel = cpal::Channel::new();

    let mut decoder = vorbis::Decoder::new(BufReader::new(include_bin!("mozart_symfony_40.ogg")))
        .unwrap();

    'main: for packet in decoder.packets() {
        let packet = packet.unwrap();
        let vorbis::Packet { channels, rate, data, .. } = packet;

        let mut data = data.iter();
        let mut next_sample = None;

        loop {
            let mut buffer = channel.append_data(channels, cpal::SamplesRate(rate as u32));
            let mut buffer = buffer.samples();

            loop {
                if next_sample.is_none() {
                    match data.next() {
                        Some(sample) => {
                            next_sample = Some(*sample as u16)
                        },
                        None => {
                            continue 'main;
                        }
                    }
                }

                if let Some(output) = buffer.next() {
                    *output = next_sample.take().unwrap();
                } else {
                    break;
                }
            }
        }
    }
}
