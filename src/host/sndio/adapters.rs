use crate::{Data, FrameCount, InputCallbackInfo, OutputCallbackInfo, Sample};
use samples_formats::TypeSampleFormat;

/// When given an input data callback that expects samples in the specified sample format, return
/// an input data callback that expects samples in the I16 sample format. The `buffer_size` is in
/// samples.
pub(super) fn input_adapter_callback<T, D>(
    mut original_data_callback: D,
    buffer_size: FrameCount,
) -> Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>
where
    D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
    T: Sample + TypeSampleFormat + Copy + Send + Default + 'static,
{
    let mut adapted_buf = vec![T::default(); buffer_size as usize];

    Box::new(move |data: &Data, info: &InputCallbackInfo| {
        let data_slice: &[i16] = data.as_slice().unwrap(); // unwrap OK because data is always i16
        let adapted_slice = &mut adapted_buf;
        assert_eq!(data_slice.len(), adapted_slice.len());
        for (adapted_ref, data_element) in adapted_slice.iter_mut().zip(data_slice.iter()) {
            *adapted_ref = T::from(data_element);
        }

        // Note: we construct adapted_data here instead of in the parent function because adapted_buf needs
        // to be owned by the closure.
        let adapted_data = unsafe { data_from_vec(&mut adapted_buf) };
        original_data_callback(&adapted_data, info);
    })
}

/// When given an output data callback that expects a place to write samples in the specified
/// sample format, return an output data callback that expects a place to write samples in the I16
/// sample format. The `buffer_size` is in samples.
pub(super) fn output_adapter_callback<T, D>(
    mut original_data_callback: D,
    buffer_size: FrameCount,
) -> Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>
where
    D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
    T: Sample + TypeSampleFormat + Copy + Send + Default + 'static,
{
    let mut adapted_buf = vec![T::default(); buffer_size as usize];
    Box::new(move |data: &mut Data, info: &OutputCallbackInfo| {
        // Note: we construct adapted_data here instead of in the parent function because
        // adapted_buf needs to be owned by the closure.
        let mut adapted_data = unsafe { data_from_vec(&mut adapted_buf) };

        // Populate adapted_buf / adapted_data.
        original_data_callback(&mut adapted_data, info);

        let data_slice: &mut [i16] = data.as_slice_mut().unwrap(); // unwrap OK because data is always i16
        let adapted_slice = &adapted_buf;
        assert_eq!(data_slice.len(), adapted_slice.len());
        for (data_ref, adapted_element) in data_slice.iter_mut().zip(adapted_slice.iter()) {
            *data_ref = adapted_element.to_i16();
        }
    })
}

unsafe fn data_from_vec<T>(adapted_buf: &mut Vec<T>) -> Data
where
    T: TypeSampleFormat,
{
    Data::from_parts(
        adapted_buf.as_mut_ptr() as *mut _,
        adapted_buf.len(), // TODO: this is converting a FrameCount to a number of samples; invalid for stereo!
        T::sample_format(),
    )
}
