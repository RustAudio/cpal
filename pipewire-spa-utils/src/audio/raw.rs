use audio::{AudioChannelPosition, AudioSampleFormat};
use format::{MediaSubtype, MediaType};
use libspa::pod::deserialize::{DeserializeError, DeserializeSuccess, ObjectPodDeserializer, PodDeserialize, PodDeserializer, Visitor};
use utils::{IdOrEnumId, IntOrChoiceInt, IntOrRangeInt32};

#[derive(Debug, Clone)]
pub struct AudioInfoRaw {
    pub media_type: MediaType,
    pub media_subtype: MediaSubtype,
    pub sample_format: IdOrEnumId<AudioSampleFormat>,
    pub sample_rate: IntOrRangeInt32,
    pub channels: IntOrChoiceInt,
    pub position: AudioChannelPosition
}

impl<'de> PodDeserialize<'de> for AudioInfoRaw {
    fn deserialize(
        deserializer: PodDeserializer<'de>,
    ) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
    where
        Self: Sized,
    {
        struct EnumFormatVisitor;
        
        impl<'de> Visitor<'de> for EnumFormatVisitor {
            type Value = AudioInfoRaw;
            type ArrayElem = std::convert::Infallible;
        
            fn visit_object(
                &self,
                object_deserializer: &mut ObjectPodDeserializer<'de>,
            ) -> Result<Self::Value, DeserializeError<&'de [u8]>> {                
                let media_type = object_deserializer
                    .deserialize_property_key(libspa::sys::SPA_FORMAT_mediaType)?
                    .0;
                let media_subtype = object_deserializer
                    .deserialize_property_key(libspa::sys::SPA_FORMAT_mediaSubtype)?
                    .0;
                let sample_format = object_deserializer
                    .deserialize_property_key(libspa::sys::SPA_FORMAT_AUDIO_format)?
                    .0;
                let sample_rate = object_deserializer
                    .deserialize_property_key(libspa::sys::SPA_FORMAT_AUDIO_rate)?
                    .0;
                let channels = object_deserializer
                    .deserialize_property_key(libspa::sys::SPA_FORMAT_AUDIO_channels)?
                    .0;
                let position = object_deserializer
                    .deserialize_property_key(libspa::sys::SPA_FORMAT_AUDIO_position)?
                    .0;
                Ok(AudioInfoRaw {
                    media_type,
                    media_subtype,
                    sample_format,
                    sample_rate,
                    channels,
                    position,
                })
            }
        }
        deserializer.deserialize_object(EnumFormatVisitor)
    }
}