use SupportedFormat;
use std::mem;
use std::ptr::null;
use std::vec::IntoIter as VecIntoIter;
use super::coreaudio::sys::{
    AudioDeviceID,
    AudioObjectPropertyAddress,
    AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize,
    kAudioHardwareNoError,
    kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMaster,
    kAudioObjectPropertyScopeGlobal,
    kAudioObjectSystemObject,
    OSStatus,
};
use super::Endpoint;

unsafe fn audio_output_devices() -> Result<Vec<AudioDeviceID>, OSStatus> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    macro_rules! try_status_or_return {
        ($status:expr) => {
            if $status != kAudioHardwareNoError as i32 {
                return Err($status);
            }
        };
    }

    let data_size = 0u32;
    let status = AudioObjectGetPropertyDataSize(
        kAudioObjectSystemObject,
        &property_address as *const _,
        0,
        null(),
        &data_size as *const _ as *mut _,
    );
    try_status_or_return!(status);

    let device_count = data_size / mem::size_of::<AudioDeviceID>() as u32;
    let mut audio_devices = vec![];
    audio_devices.reserve_exact(device_count as usize);

    let status = AudioObjectGetPropertyData(
        kAudioObjectSystemObject,
        &property_address as *const _,
        0,
        null(),
        &data_size as *const _ as *mut _,
        audio_devices.as_mut_ptr() as *mut _,
    );
    try_status_or_return!(status);

    audio_devices.set_len(device_count as usize);

    // Only keep the devices that have some supported output format.
    audio_devices.retain(|&id| {
        let e = Endpoint { audio_device_id: id };
        match e.supported_formats() {
            Err(_) => false,
            Ok(mut fmts) => fmts.next().is_some(),
        }
    });

    Ok(audio_devices)
}

pub struct EndpointsIterator(VecIntoIter<AudioDeviceID>);

unsafe impl Send for EndpointsIterator {
}
unsafe impl Sync for EndpointsIterator {
}

impl Default for EndpointsIterator {
    fn default() -> Self {
        let devices = unsafe {
            audio_output_devices().expect("failed to get audio output devices")
        };
        EndpointsIterator(devices.into_iter())
    }
}

impl Iterator for EndpointsIterator {
    type Item = Endpoint;
    fn next(&mut self) -> Option<Endpoint> {
        self.0.next().map(|id| Endpoint { audio_device_id: id })
    }
}

pub fn default_endpoint() -> Option<Endpoint> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let audio_device_id: AudioDeviceID = 0;
    let data_size = mem::size_of::<AudioDeviceID>();;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &property_address as *const _,
            0,
            null(),
            &data_size as *const _ as *mut _,
            &audio_device_id as *const _ as *mut _,
        )
    };
    if status != kAudioHardwareNoError as i32 {
        return None;
    }

    let endpoint = Endpoint {
        audio_device_id: audio_device_id,
    };
    Some(endpoint)
}

pub type SupportedFormatsIterator = VecIntoIter<SupportedFormat>;
