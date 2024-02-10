#[cfg(test)]
#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    feature = "wasm-bindgen-test"
))]
pub mod tests {
    use cpal::platform::atomic_buffer::*;
    use rand::Rng;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn it_creates_with_max_chunks() {
        AtomicBuffer::new(8_323_580, 128);
    }

    #[wasm_bindgen_test]
    fn simple_sequence_wrote_and_read() {
        console_log::init().unwrap();

        let buffer = AtomicBuffer::new(8, 256);
        let mut rng = rand::thread_rng();
        let chunked_data = (0..8)
            .map(|_| {
                (0..256)
                    .map(|_| rng.gen_range(-1.0..=1.0))
                    .collect::<Vec<f32>>()
            })
            .collect::<Vec<_>>();

        for chunk in chunked_data.iter() {
            buffer.write(chunk.as_slice()).expect("write buffer");
        }

        let mut data_from_buffer = (0..8).map(|_| vec![0_f32; 256]).collect::<Vec<_>>();
        for (_, chunk) in data_from_buffer.iter_mut().enumerate() {
            buffer.read(chunk.as_mut_slice()).expect("read buffer")
        }

        assert_eq!(
            chunked_data, data_from_buffer,
            "data is the same after coming through buffer"
        );

        let all = data_from_buffer
            .into_iter()
            .flat_map(|v| v)
            .collect::<Vec<_>>();

        assert!(
            all.into_iter().any(|v| v != 0.0_f32),
            "random values aren't all zeroes"
        );
    }

    #[wasm_bindgen_test]
    fn chaotic_write_read() {
        let buffer = AtomicBuffer::new(4, 128);
        let mut rng = rand::thread_rng();
        let chunked_data = (0..12)
            .map(|_| {
                (0..128)
                    .map(|_| rng.gen_range(-1.0..=1.0))
                    .collect::<Vec<f32>>()
            })
            .collect::<Vec<_>>();

        let mut data_from_buffer = (0..12).map(|_| vec![0_f32; 128]).collect::<Vec<_>>();

        // +4
        buffer
            .write(chunked_data[0].as_slice())
            .expect("write chunk");
        buffer
            .write(chunked_data[1].as_slice())
            .expect("write chunk");
        buffer
            .write(chunked_data[2].as_slice())
            .expect("write chunk");
        buffer
            .write(chunked_data[3].as_slice())
            .expect("write chunk");

        assert_eq!(buffer.chunks_to_read_count(), 4);
        assert_eq!(buffer.chunks_to_write_count(), 0);

        // -2
        buffer
            .read(data_from_buffer[0].as_mut_slice())
            .expect("read chunk");
        buffer
            .read(data_from_buffer[1].as_mut_slice())
            .expect("read chunk");

        assert_eq!(buffer.chunks_to_read_count(), 2);
        assert_eq!(buffer.chunks_to_write_count(), 2);

        // +2
        buffer
            .write(chunked_data[4].as_slice())
            .expect("write chunk");
        buffer
            .write(chunked_data[5].as_slice())
            .expect("write chunk");

        assert_eq!(buffer.chunks_to_read_count(), 4);
        assert_eq!(buffer.chunks_to_write_count(), 0);

        // -4
        buffer
            .read(data_from_buffer[2].as_mut_slice())
            .expect("read chunk");
        buffer
            .read(data_from_buffer[3].as_mut_slice())
            .expect("read chunk");
        buffer
            .read(data_from_buffer[4].as_mut_slice())
            .expect("read chunk");
        buffer
            .read(data_from_buffer[5].as_mut_slice())
            .expect("read chunk");

        assert_eq!(buffer.chunks_to_read_count(), 0);
        assert_eq!(buffer.chunks_to_write_count(), 4);

        // +2
        buffer
            .write(chunked_data[6].as_slice())
            .expect("write chunk");
        buffer
            .write(chunked_data[7].as_slice())
            .expect("write chunk");

        assert_eq!(buffer.chunks_to_read_count(), 2);
        assert_eq!(buffer.chunks_to_write_count(), 2);

        // -2
        buffer
            .read(data_from_buffer[6].as_mut_slice())
            .expect("read chunk");
        buffer
            .read(data_from_buffer[7].as_mut_slice())
            .expect("read chunk");

        assert_eq!(buffer.chunks_to_read_count(), 0);
        assert_eq!(buffer.chunks_to_write_count(), 4);

        // +3
        buffer
            .write(chunked_data[8].as_slice())
            .expect("write chunk");
        buffer
            .write(chunked_data[9].as_slice())
            .expect("write chunk");
        buffer
            .write(chunked_data[10].as_slice())
            .expect("write chunk");

        assert_eq!(buffer.chunks_to_read_count(), 3);
        assert_eq!(buffer.chunks_to_write_count(), 1);

        // -1
        buffer
            .read(data_from_buffer[8].as_mut_slice())
            .expect("read chunk");

        assert_eq!(buffer.chunks_to_read_count(), 2);
        assert_eq!(buffer.chunks_to_write_count(), 2);

        // +1
        buffer
            .write(chunked_data[11].as_slice())
            .expect("write chunk");

        assert_eq!(buffer.chunks_to_read_count(), 3);
        assert_eq!(buffer.chunks_to_write_count(), 1);

        // -4
        buffer
            .read(data_from_buffer[9].as_mut_slice())
            .expect("read chunk");
        buffer
            .read(data_from_buffer[10].as_mut_slice())
            .expect("read chunk");
        buffer
            .read(data_from_buffer[11].as_mut_slice())
            .expect("read chunk");

        assert_eq!(buffer.chunks_to_read_count(), 0);
        assert_eq!(buffer.chunks_to_write_count(), 4);


        assert_eq!(
            chunked_data, data_from_buffer,
            "data is the same after coming through buffer"
        );
    }
}
