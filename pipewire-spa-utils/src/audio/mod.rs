use libspa::pod::deserialize::DeserializeError;
use libspa::pod::deserialize::DeserializeSuccess;
use libspa::pod::deserialize::PodDeserialize;
use libspa::pod::deserialize::PodDeserializer;
use libspa::pod::deserialize::VecVisitor;
use libspa::utils::Id;
use std::convert::TryInto;
use std::ops::Deref;
use impl_array_id_deserializer;
use utils::IdOrEnumId;

pub mod raw;

include!(concat!(env!("OUT_DIR"), "/audio.rs"));

#[derive(Debug, Clone)]
pub struct AudioSampleFormatEnum(IdOrEnumId<AudioSampleFormat>);

impl Deref for AudioSampleFormatEnum {
    type Target = IdOrEnumId<AudioSampleFormat>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct AudioChannelPosition(Vec<AudioChannel>);

impl Default for AudioChannelPosition {
    fn default() -> Self {
        AudioChannelPosition(vec![])
    }
}

impl AudioChannelPosition {
    pub fn to_array<const N: usize>(&self) -> [u32; N] {
        let mut channels = self.0
            .iter()
            .map(move |channel| *channel as u32)
            .collect::<Vec<u32>>();
        if channels.len() < N {
            channels.resize(N, AudioChannel::UNKNOWN as u32);
        }
        channels.try_into().unwrap()
    }
}

impl Deref for AudioChannelPosition {
    type Target = Vec<AudioChannel>;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl_array_id_deserializer!(AudioChannelPosition, AudioChannel);