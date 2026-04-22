use std::{
    mem,
    ptr::{null, NonNull},
    vec::IntoIter as VecIntoIter,
};

use objc2_core_audio::{
    kAudioHardwareNoError, kAudioHardwarePropertyDefaultInputDevice,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioDeviceID, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress,
};

use super::{check_os_status, Device};
pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};
use crate::Error;

unsafe fn audio_devices() -> Result<Vec<AudioDeviceID>, Error> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    macro_rules! try_status_or_return {
        ($status:expr) => {
            if $status != kAudioHardwareNoError as i32 {
                return Err(check_os_status($status).unwrap_err());
            }
        };
    }

    let mut data_size = 0u32;
    let status = AudioObjectGetPropertyDataSize(
        kAudioObjectSystemObject as AudioObjectID,
        NonNull::from(&property_address),
        0,
        null(),
        NonNull::from(&mut data_size),
    );
    try_status_or_return!(status);

    let device_count = data_size / mem::size_of::<AudioDeviceID>() as u32;
    let mut audio_devices = vec![];
    audio_devices.reserve_exact(device_count as usize);

    let status = AudioObjectGetPropertyData(
        kAudioObjectSystemObject as AudioObjectID,
        NonNull::from(&property_address),
        0,
        null(),
        NonNull::from(&mut data_size),
        NonNull::new(audio_devices.as_mut_ptr()).unwrap().cast(),
    );
    try_status_or_return!(status);

    audio_devices.set_len(device_count as usize);

    Ok(audio_devices)
}

pub struct Devices(VecIntoIter<AudioDeviceID>);

impl Devices {
    pub fn new() -> Result<Self, Error> {
        let devices = unsafe { audio_devices() }?;
        Ok(Self(devices.into_iter()))
    }
}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|id| Self::Item {
            audio_device_id: id,
        })
    }
}

pub fn default_input_device() -> Option<Device> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut audio_device_id: AudioDeviceID = 0;
    let data_size = mem::size_of::<AudioDeviceID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&property_address),
            0,
            null(),
            NonNull::from(&data_size),
            NonNull::from(&mut audio_device_id).cast(),
        )
    };
    if status != kAudioHardwareNoError {
        return None;
    }

    let device = Device { audio_device_id };
    Some(device)
}

pub fn default_output_device() -> Option<Device> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let mut audio_device_id: AudioDeviceID = 0;
    let data_size = mem::size_of::<AudioDeviceID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&property_address),
            0,
            null(),
            NonNull::from(&data_size),
            NonNull::from(&mut audio_device_id).cast(),
        )
    };
    if status != kAudioHardwareNoError {
        return None;
    }

    let device = Device { audio_device_id };
    Some(device)
}
