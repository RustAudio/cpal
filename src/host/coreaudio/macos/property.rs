//! Helper code for reading CoreAudio object properties.
use std::{
    mem,
    ptr::{NonNull, null},
};

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress,
};

/// Read a single fixed-size property value.
///
/// # Safety
///
/// `T` must match the binary layout CoreAudio writes for `address`.
pub unsafe fn get_property<T>(
    object_id: AudioObjectID,
    address: AudioObjectPropertyAddress,
) -> Result<T, coreaudio::Error> {
    let mut value = mem::MaybeUninit::<T>::zeroed();
    let mut data_size = mem::size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut value).cast(),
        )
    };
    coreaudio::Error::from_os_status(status)?;
    Ok(unsafe { value.assume_init() })
}

/// Read a variable-length array property.
///
/// # Safety
///
/// `T` must match the binary layout CoreAudio writes for each element of `address`.
pub unsafe fn get_property_array<T>(
    object_id: AudioObjectID,
    address: AudioObjectPropertyAddress,
) -> Result<Vec<T>, coreaudio::Error> {
    let mut data_size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut data_size),
        )
    };
    coreaudio::Error::from_os_status(status)?;

    let n = data_size as usize / mem::size_of::<T>();
    let mut values: Vec<T> = Vec::with_capacity(n);
    let status = unsafe {
        AudioObjectGetPropertyData(
            object_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut data_size),
            NonNull::new(values.as_mut_ptr()).unwrap().cast(),
        )
    };
    coreaudio::Error::from_os_status(status)?;
    // SAFETY: the size query above reported room for exactly `n` elements, and the status check
    // confirms CoreAudio filled the buffer it was given.
    unsafe { values.set_len(n) };
    Ok(values)
}
