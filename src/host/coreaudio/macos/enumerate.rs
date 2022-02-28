extern crate coreaudio;

use self::coreaudio::sys::{
    kAudioHardwareNoError, kAudioHardwarePropertyDefaultInputDevice,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioDeviceID, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectPropertyAddress, OSStatus,
};
use super::Device;
use crate::{BackendSpecificError, DevicesError, SupportedStreamConfigRange};
use std::mem;
use std::ptr::null;
use std::vec::IntoIter as VecIntoIter;

unsafe fn audio_devices() -> Result<Vec<AudioDeviceID>, OSStatus> {
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

    Ok(audio_devices)
}

pub struct Devices(VecIntoIter<AudioDeviceID>);

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        let devices = unsafe {
            match audio_devices() {
                Ok(devices) => devices,
                Err(os_status) => {
                    let description = format!("{}", os_status);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            }
        };
        Ok(Devices(devices.into_iter()))
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

impl Iterator for Devices {
    type Item = Device;
    fn next(&mut self) -> Option<Device> {
        self.0.next().map(|id| Device {
            audio_device_id: id,
            is_default: false,
        })
    }
}

pub fn default_input_device() -> Option<Device> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let audio_device_id: AudioDeviceID = 0;
    let data_size = mem::size_of::<AudioDeviceID>();
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

    let device = Device {
        audio_device_id,
        is_default: true,
    };
    Some(device)
}

pub fn default_output_device() -> Option<Device> {
    let property_address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };

    let audio_device_id: AudioDeviceID = 0;
    let data_size = mem::size_of::<AudioDeviceID>();
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

    let device = Device {
        audio_device_id,
        is_default: true,
    };
    Some(device)
}

pub type SupportedInputConfigs = VecIntoIter<SupportedStreamConfigRange>;
pub type SupportedOutputConfigs = VecIntoIter<SupportedStreamConfigRange>;
