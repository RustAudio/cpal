use pipewire_spa_utils::audio::{AudioChannelPosition};
use pipewire_spa_utils::audio::AudioSampleFormat;
use pipewire_spa_utils::audio::raw::AudioInfoRaw;
use pipewire_spa_utils::format::{MediaSubtype, MediaType};
use crate::utils::Direction;

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub nickname: String,
    pub direction: Direction,
    pub is_default: bool,
    pub format: AudioInfoRaw
}

#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    pub media_type: MediaType,
    pub media_subtype: MediaSubtype,
    pub sample_format: AudioSampleFormat,
    pub sample_rate: u32,
    pub channels: u32,
    pub position: AudioChannelPosition
}

impl From<AudioInfoRaw> for AudioStreamInfo {
    fn from(value: AudioInfoRaw) -> Self {
        Self {
            media_type: MediaType::Audio,
            media_subtype: MediaSubtype::Raw,
            sample_format: value.sample_format.default,
            sample_rate: value.sample_rate.value,
            channels: *value.channels,
            position: AudioChannelPosition::default(),
        }
    }
}

impl From<AudioStreamInfo> for pipewire::spa::param::audio::AudioInfoRaw {
    fn from(value: AudioStreamInfo) -> Self {
        let format: pipewire::spa::sys::spa_audio_format = value.sample_format as u32;
        let format = pipewire::spa::param::audio::AudioFormat::from_raw(format);
        let position: [u32; 64] = value.position.to_array();
        let mut info = pipewire::spa::param::audio::AudioInfoRaw::default();
        info.set_format(format);
        info.set_rate(value.sample_rate);
        info.set_channels(value.channels);
        info.set_position(position);
        info
    }
}