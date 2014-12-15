extern crate cpal;
extern crate vorbis;

use std::io::BufReader;

fn main() {
    let mut channel = cpal::Channel::new();

    let mut decoder = vorbis::Decoder::new(BufReader::new(include_bin!("mozart_symfony_40.ogg")))
        .unwrap();

    for packet in decoder.packets() {
        let packet = packet.unwrap();

        let mut buffer = channel.append_data(packet.channels, cpal::SamplesRate(packet.rate as u32));

        // FIXME: data loss
        for (i, o) in packet.data.into_iter().zip(buffer.iter_mut()) {
            *o = i as u16;
        }
    }
}
