fn main() {
    let input_device = cpal::default_input_device().unwrap();
    let input_format = input_device.default_input_format().unwrap();

    let event_loop = cpal::EventLoop::new();

    let input_stream_id = event_loop.build_input_stream(&input_device, &input_format).unwrap();
    event_loop.play_stream(input_stream_id);

    event_loop.run(move |_stream_id, stream_data| {
        match stream_data {
            cpal::StreamData::Input {
                buffer: cpal::UnknownTypeInputBuffer::F32(_buffer),
            } => {
                // not even use buffer
            }
            _ => unreachable!(),
        }
    });
}
