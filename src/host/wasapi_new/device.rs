use super::Device;
use super::utils::WindowsError;
use crate::DeviceDirection;
use itertools::Itertools;
use tap::Pipe;
use windows::Devices::Enumeration::DeviceInformation;
use windows::Media::Devices::{AudioDeviceRole, MediaDevice};
use windows_collections::IIterable;
use windows_core::HSTRING;

fn extra_properties() -> IIterable<HSTRING> {
    todo!()
}

pub fn all() -> Result<Vec<Device>, WindowsError> {
    let extra_properties = extra_properties();

    let capture = all_in_direction::<true>(&extra_properties)?;
    let render  = all_in_direction::<false>(&extra_properties)?;

    capture
        .chain(render)
        .collect_vec()
        .pipe(Ok)
}

fn all_in_direction<const INPUT: bool>(extra_props: &IIterable<HSTRING>) -> Result<impl Iterator<Item = Device>, WindowsError> {
    let fn_get_selector =
        if INPUT { MediaDevice::GetAudioCaptureSelector }
        else     { MediaDevice::GetAudioRenderSelector };

    DeviceInformation
        ::FindAllAsyncAqsFilterAndAdditionalProperties(&fn_get_selector()?, extra_props)?
        .join()?
        .into_iter()
        .map(|info| Device { info, direction: direction::<INPUT>() })
        .pipe(Ok)
}

pub fn default<const INPUT: bool>() -> Result<Device, WindowsError> {
    let extra_properties = extra_properties();

    let fn_get_id =
        if INPUT { MediaDevice::GetDefaultAudioCaptureId }
        else     { MediaDevice::GetDefaultAudioRenderId };

    fn_get_id(AudioDeviceRole::Default)?
        .pipe_ref(|id| DeviceInformation::CreateFromIdAsyncAdditionalProperties(id, &extra_properties))?
        .join()
        .map(|info| Device { info, direction: direction::<INPUT>() })
}

const fn direction<const INPUT: bool>() -> DeviceDirection {
    if INPUT { DeviceDirection::Input }
    else     { DeviceDirection::Output }
}