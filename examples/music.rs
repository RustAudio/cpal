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

        loop {
            let mut buffer = channel.append_data(channels, cpal::SamplesRate(rate as u32));
            let mut buffer = buffer.samples();

            for output in buffer {
                match data.next() {
                    Some(sample) => {
                        *output = *sample as u16;
                    },
                    None => {
                        continue 'main;
                    }
                }
            }
        }
    }
}
