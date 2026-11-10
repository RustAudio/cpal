use super::{Device, utils::WindowsError};
use crate::{DeviceDirection, DeviceType, InterfaceType};
use itertools::Itertools;
use tap::Pipe;
use windows::{
    Devices::Enumeration::DeviceInformation,
    Foundation::IPropertyValue,
    Media::Devices::{AudioDeviceRole, MediaDevice},
    Win32::Foundation::{PROPERTYKEY, S_OK},
    Win32::Media::Audio::{self, *},
};
use windows_collections::{IIterable, IMapView};
use windows_core::{Array, HSTRING, IInspectable, Interface, h};

pub fn format_aqs(PROPERTYKEY { fmtid, pid }: &PROPERTYKEY) -> HSTRING {
    format!("{{{fmtid:?}}} {pid}").into()
}

fn extra_properties() -> IIterable<HSTRING> {
    vec![
        format_aqs(&PKEY_AudioEndpoint_FormFactor),
        format_aqs(&PKEY_AudioEndpoint_JackSubType),
    ]
    .into()
}

pub fn all() -> Result<Vec<Device>, WindowsError> {
    let extra_properties = extra_properties();

    let capture = all_in_direction::<true >(&extra_properties)?;
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

const PHYSICAL_DEVICE_LOCATION: &HSTRING = h!("System.Devices.PhysicalDeviceLocation");

pub struct PropertyReader(pub IMapView<HSTRING, IInspectable>);

impl PropertyReader {
    pub fn types(&self) -> Result<(DeviceType, InterfaceType), WindowsError> {
        let form_factor = self
            .0
            .Lookup(&format_aqs(&PKEY_AudioEndpoint_FormFactor))?
            .cast::<IPropertyValue>()?
            .GetInt32()?
            .pipe(EndpointFormFactor);

        let mut device_type    = DeviceType   ::Unknown;
        let mut interface_type = InterfaceType::Unknown;

        #[deny(nonstandard_style, reason = "prevent accidental wildcards")]
        #[expect(unused_qualifications, reason = "rustc bug?")]
        match form_factor {
            Audio::DigitalAudioDisplayDevice => interface_type = InterfaceType::DisplayPort,
            Audio::UnknownDigitalPassthrough => interface_type = InterfaceType::Virtual,
            Audio::RemoteNetworkDevice       => interface_type = InterfaceType::Network,
            Audio::UnknownFormFactor         => interface_type = InterfaceType::Unknown,
            Audio::LineLevel                 => interface_type = InterfaceType::Line,
            Audio::SPDIF                     => interface_type = InterfaceType::Spdif,
            Audio::Headphones                => device_type    = DeviceType::Headphones,
            Audio::Microphone                => device_type    = DeviceType::Microphone,
            Audio::Speakers                  => device_type    = DeviceType::Speaker,
            Audio::Headset                   => device_type    = DeviceType::Headset,
            Audio::Handset                   => device_type    = DeviceType::Handset,
            _                                => unreachable!()
        };

        let jack_sub_type = self
            .0
            .Lookup(&format_aqs(&PKEY_AudioEndpoint_JackSubType))?
            .cast::<IPropertyValue>()?
            .GetString()?
            .to_string_lossy()
            .trim_matches(['{', '}'])
            .try_into()?;

        use windows::Win32::Media::KernelStreaming::*;
        #[deny(nonstandard_style, reason = "prevent accidental wildcards")]
        match jack_sub_type {
            KSNODETYPE_BIDIRECTIONAL_UNDEFINED        => interface_type = InterfaceType::Aggregate,
            KSNODETYPE_DISPLAYPORT_INTERFACE          => interface_type = InterfaceType::DisplayPort,
            KSNODETYPE_SPDIF_INTERFACE                => interface_type = InterfaceType::Spdif,
            KSNODETYPE_HDMI_INTERFACE                 => interface_type = InterfaceType::Hdmi,
            
            KSNODETYPE_AUDIO_LOOPBACK                 |
            KSNODETYPE_AUDIO_ENGINE                   => interface_type = InterfaceType::Virtual,
    
            KSNODETYPE_ANALOG_CONNECTOR               |
            KSNODETYPE_LINE_CONNECTOR                 |
            KSNODETYPE_PHONE_LINE                     |
            KSNODETYPE_ADC                            |
            KSNODETYPE_DAC                            => interface_type = InterfaceType::Line,

            KSNODETYPE_DIGITAL_AUDIO_INTERFACE        => device_type    = DeviceType::Dock,
            KSNODETYPE_HEADPHONES                     => device_type    = DeviceType::Headphones,
            
            KSNODETYPE_TV_TUNER_AUDIO                 |
            KSNODETYPE_CABLE_TUNER_AUDIO              => device_type    = DeviceType::Tuner,

            KSNODETYPE_HEADSET                        |
            KSNODETYPE_HEAD_MOUNTED_DISPLAY_AUDIO     => device_type    = DeviceType::Headset,
            
            
            KSNODETYPE_TELEPHONY_UNDEFINED            |
            KSNODETYPE_DOWN_LINE_PHONE                |
            KSNODETYPE_TELEPHONY_BIDI                 |
            KSNODETYPE_TELEPHONE                      |
            KSNODETYPE_HANDSET                        => device_type    = DeviceType::Handset,
            
            KSNODETYPE_OMNI_DIRECTIONAL_MICROPHONE    |
            KSNODETYPE_PROCESSING_MICROPHONE_ARRAY    |
            KSNODETYPE_PERSONAL_MICROPHONE            |
            KSNODETYPE_DESKTOP_MICROPHONE             |
            KSNODETYPE_MICROPHONE_ARRAY               |
            KSNODETYPE_MICROPHONE                     => device_type    = DeviceType::Microphone,

            KSNODETYPE_SPEAKERPHONE_NO_ECHO_REDUCTION |
            KSNODETYPE_ECHO_SUPPRESSING_SPEAKERPHONE  |
            KSNODETYPE_LOW_FREQUENCY_EFFECTS_SPEAKER  |
            KSNODETYPE_ECHO_CANCELING_SPEAKERPHONE    |
            KSNODETYPE_COMMUNICATION_SPEAKER          |
            KSNODETYPE_SPEAKERS_STATIC_JACK           |
            KSNODETYPE_DESKTOP_SPEAKER                |
            KSNODETYPE_ROOM_SPEAKER                   |
            KSNODETYPE_PHONOGRAPH                     |
            KSNODETYPE_SPEAKER                        => device_type    = DeviceType::Speaker,

            _ => ()
        }

        Ok((device_type, interface_type))
    }

    pub fn physical_location(&self) -> Result<Option<String>, WindowsError> {
        match self.0.Lookup(PHYSICAL_DEVICE_LOCATION) {
            Err(error) if error.code() == S_OK => Ok(None), // https://github.com/microsoft/windows-rs/issues/4858

            Err(error) => Err(error),

            Ok(inspectable) => {
                let mut buf = Array::new();
                inspectable
                    .cast::<IPropertyValue>()?
                    .GetUInt8Array(&mut buf)?;
                Ok(Some(format!("{:x?}", buf.as_slice())))
            }
        }
    }
}
