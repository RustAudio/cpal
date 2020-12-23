use crate::{Data, FrameCount, InputCallbackInfo, OutputCallbackInfo, Sample, SampleFormat};

/// When given an input data callback that expects samples in the specified sample format, return
/// an input data callback that expects samples in the I16 sample format. The `buffer_size` is in
/// samples.
pub(super) fn input_adapter_callback<D>(
    mut original_data_callback: D,
    buffer_size: FrameCount,
    sample_format: SampleFormat,
) -> Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
{
    match sample_format {
        SampleFormat::I16 => {
            // no-op
            return Box::new(original_data_callback);
        }
        SampleFormat::F32 => {
            // Make the backing buffer for the Data used in the closure.
            let mut adapted_buf = vec![0f32].repeat(buffer_size as usize);
            Box::new(move |data: &Data, info: &InputCallbackInfo| {
                let data_slice: &[i16] = data.as_slice().unwrap(); // unwrap OK because data is always i16
                let adapted_slice = &mut adapted_buf;
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, adapted_ref) in adapted_slice.iter_mut().enumerate() {
                    *adapted_ref = data_slice[i].to_f32();
                }

                // Note: we construct adapted_data here instead of in the parent function because adapted_buf needs
                // to be owned by the closure.
                let adapted_data = unsafe {
                    Data::from_parts(
                        adapted_buf.as_mut_ptr() as *mut _,
                        buffer_size as usize, // TODO: this is converting a FrameCount to a number of samples; invalid for stereo!
                        sample_format,
                    )
                };
                original_data_callback(&adapted_data, info);
            })
        }
        SampleFormat::U16 => {
            let mut adapted_buf = vec![0u16].repeat(buffer_size as usize);
            Box::new(move |data: &Data, info: &InputCallbackInfo| {
                let data_slice: &[i16] = data.as_slice().unwrap(); // unwrap OK because data is always i16
                let adapted_slice = &mut adapted_buf;
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, adapted_ref) in adapted_slice.iter_mut().enumerate() {
                    *adapted_ref = data_slice[i].to_u16();
                }

                // Note: we construct adapted_data here instead of in the parent function because adapted_buf needs
                // to be owned by the closure.
                let adapted_data = unsafe {
                    Data::from_parts(
                        adapted_buf.as_mut_ptr() as *mut _,
                        buffer_size as usize, // TODO: this is converting a FrameCount to a number of samples; invalid for stereo!
                        sample_format,
                    )
                };
                original_data_callback(&adapted_data, info);
            })
        }
    }
}

/// When given an output data callback that expects a place to write samples in the specified
/// sample format, return an output data callback that expects a place to write samples in the I16
/// sample format. The `buffer_size` is in samples.
pub(super) fn output_adapter_callback<D>(
    mut original_data_callback: D,
    buffer_size: FrameCount,
    sample_format: SampleFormat,
) -> Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
{
    match sample_format {
        SampleFormat::I16 => {
            // no-op
            return Box::new(original_data_callback);
        }
        SampleFormat::F32 => {
            // Make the backing buffer for the Data used in the closure.
            let mut adapted_buf = vec![0f32].repeat(buffer_size as usize);

            Box::new(move |data: &mut Data, info: &OutputCallbackInfo| {
                // Note: we construct adapted_data here instead of in the parent function because
                // adapted_buf needs to be owned by the closure.
                let mut adapted_data = unsafe {
                    Data::from_parts(
                        adapted_buf.as_mut_ptr() as *mut _,
                        buffer_size as usize, // TODO: this is converting a FrameCount to a number of samples; invalid for stereo!
                        sample_format,
                    )
                };

                // Populate adapted_buf / adapted_data.
                original_data_callback(&mut adapted_data, info);

                let data_slice: &mut [i16] = data.as_slice_mut().unwrap(); // unwrap OK because data is always i16
                let adapted_slice = &adapted_buf;
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, data_ref) in data_slice.iter_mut().enumerate() {
                    *data_ref = adapted_slice[i].to_i16();
                }
            })
        }
        SampleFormat::U16 => {
            // Make the backing buffer for the Data used in the closure.
            let mut adapted_buf = vec![0u16].repeat(buffer_size as usize);

            Box::new(move |data: &mut Data, info: &OutputCallbackInfo| {
                // Note: we construct adapted_data here instead of in the parent function because
                // adapted_buf needs to be owned by the closure.
                let mut adapted_data = unsafe {
                    Data::from_parts(
                        adapted_buf.as_mut_ptr() as *mut _,
                        buffer_size as usize, // TODO: this is converting a FrameCount to a number of samples; invalid for stereo!
                        sample_format,
                    )
                };

                // Populate adapted_buf / adapted_data.
                original_data_callback(&mut adapted_data, info);

                let data_slice: &mut [i16] = data.as_slice_mut().unwrap(); // unwrap OK because data is always i16
                let adapted_slice = &adapted_buf;
                assert_eq!(data_slice.len(), adapted_slice.len());
                for (i, data_ref) in data_slice.iter_mut().enumerate() {
                    *data_ref = adapted_slice[i].to_i16();
                }
            })
        }
    }
}
