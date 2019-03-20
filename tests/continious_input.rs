#[test]
#[cfg_attr(not(feature = "expensive_tests"), ignore)]  // this test waits for 10 minutes
fn long_input() {
    // Prepare test conditions.

    let input_device = cpal::default_input_device().unwrap();
    let input_format = input_device.default_input_format().unwrap();

    let event_loop = cpal::EventLoop::new();

    let input_stream_id = event_loop.build_input_stream(&input_device, &input_format).unwrap();
    event_loop.play_stream(input_stream_id);

    let working = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let working_consumer = working.clone();

    // Simulate using data by appending it to the buffer...
    let io_buffer = std::collections::VecDeque::<f32>::with_capacity(30_000_000);
    let mu = std::sync::Mutex::new(io_buffer);

    // Simulate real-world usage by executing event-loop in a separate thread.
    std::thread::spawn(move || {
        event_loop.run(move |_stream_id, stream_data| {
            if !working_consumer.load(std::sync::atomic::Ordering::Relaxed) {
                let io_buffer = mu.lock().unwrap();
                dbg!(io_buffer.len());
                return;
            }

            match stream_data {
                cpal::StreamData::Input {
                    buffer: cpal::UnknownTypeInputBuffer::F32(buffer),
                } => {
                    let mut io_buffer = mu.lock().unwrap();
                    io_buffer.extend(buffer.into_iter());

                    // Clear the buffer so that we don't run out of memory.
                    io_buffer.clear();
                }

                _ => panic!("unhandled stream data"),
            }
        });
    });

    // Run the test for 600 seconds to make sure we don't crash.
    std::thread::sleep(std::time::Duration::from_secs(600));

    // Stop the event loop.
    working.store(false, std::sync::atomic::Ordering::Relaxed);
}
