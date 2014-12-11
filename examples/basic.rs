extern crate cpal;

fn main() {
    let mut channel = cpal::Channel::new();

    loop {
        let mut buffer = channel.append_data::<(u16, u16)>();
        buffer[0] = std::rand::random();
    }
}
