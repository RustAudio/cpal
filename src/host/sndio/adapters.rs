use crate::{Data, InputCallbackInfo, OutputCallbackInfo, Sample, SampleFormat};

/// When given an input data callback that expects samples in the specified sample format, return
/// an input data callback that expects samples in the I16 sample format. The `buffer_size` is in
/// samples.
pub(super) fn input_adapter_callback<D>(
    mut original_data_callback: D,
    buffer_size: usize,
    sample_format: SampleFormat,
) -> Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
{
    if sample_format == SampleFormat::I16 {
        // no-op
        return Box::new(original_data_callback);
    }

    // Make the backing buffer for the Data used in the closure.
    let mut buf: Vec<u8> = vec![0].repeat(buffer_size * sample_format.sample_size());

    Box::new(move |data: &Data, info: &InputCallbackInfo| {
        // Note: we construct adapted_data here instead of in the parent function because buf needs
        // to be owned by the closure.
        let mut adapted_data =
            unsafe { Data::from_parts(buf.as_mut_ptr() as *mut _, buffer_size, sample_format) };
        let data_slice: &[i16] = data.as_slice().unwrap(); // unwrap OK because data is always i16
        match sample_format {
            SampleFormat::F32 => {
                let adapted_slice: &mut [f32] = adapted_data.as_slice_mut().unwrap(); // unwrap OK because of the match
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, adapted_ref) in adapted_slice.iter_mut().enumerate() {
                    *adapted_ref = data_slice[i].to_f32();
                }
            }
            SampleFormat::U16 => {
                let adapted_slice: &mut [u16] = adapted_data.as_slice_mut().unwrap(); // unwrap OK because of the match
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, adapted_ref) in adapted_slice.iter_mut().enumerate() {
                    *adapted_ref = data_slice[i].to_u16();
                }
            }
            SampleFormat::I16 => {
                unreachable!("i16 should've already been handled above");
            }
        }
        original_data_callback(&adapted_data, info);
    })
}

/// When given an output data callback that expects a place to write samples in the specified
/// sample format, return an output data callback that expects a place to write samples in the I16
/// sample format. The `buffer_size` is in samples.
pub(super) fn output_adapter_callback<D>(
    mut original_data_callback: D,
    buffer_size: usize,
    sample_format: SampleFormat,
) -> Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
{
    if sample_format == SampleFormat::I16 {
        // no-op
        return Box::new(original_data_callback);
    }

    // Make the backing buffer for the Data used in the closure.
    let mut buf: Vec<u8> = vec![0].repeat(buffer_size * sample_format.sample_size());

    Box::new(move |data: &mut Data, info: &OutputCallbackInfo| {
        // Note: we construct adapted_data here instead of in the parent function because buf needs
        // to be owned by the closure.
        let mut adapted_data =
            unsafe { Data::from_parts(buf.as_mut_ptr() as *mut _, buffer_size, sample_format) };

        // Populate buf / adapted_data.
        original_data_callback(&mut adapted_data, info);

        let data_slice: &mut [i16] = data.as_slice_mut().unwrap(); // unwrap OK because data is always i16
        match sample_format {
            SampleFormat::F32 => {
                let adapted_slice: &[f32] = adapted_data.as_slice().unwrap(); // unwrap OK because of the match
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, data_ref) in data_slice.iter_mut().enumerate() {
                    *data_ref = adapted_slice[i].to_i16();
                }
            }
            SampleFormat::U16 => {
                let adapted_slice: &[u16] = adapted_data.as_slice().unwrap(); // unwrap OK because of the match
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, data_ref) in data_slice.iter_mut().enumerate() {
                    *data_ref = adapted_slice[i].to_i16();
                }
            }
            SampleFormat::I16 => {
                unreachable!("i16 should've already been handled above");
            }
        }
    })
}
