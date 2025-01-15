use pipewire_client::spa_utils::audio::{AudioChannelPosition, AudioSampleFormat, AudioSampleFormatEnum};
use pipewire_client::spa_utils::format::{MediaType, MediaSubtype};
use pipewire_client::{pipewire, AudioStreamInfo};
use crate::{BackendSpecificError, ChannelCount, Data, SampleFormat, StreamConfig};

impl TryFrom<SampleFormat> for AudioSampleFormat {
    type Error = BackendSpecificError;

    fn try_from(value: SampleFormat) -> Result<Self, Self::Error> {
        let value = match value {
            SampleFormat::I8 => AudioSampleFormat::S8,
            SampleFormat::U8 => AudioSampleFormat::U8,
            SampleFormat::I16 => AudioSampleFormat::S16_LE,
            SampleFormat::U16 => AudioSampleFormat::U16_LE,
            SampleFormat::I32 => AudioSampleFormat::S32_LE,
            SampleFormat::U32 => AudioSampleFormat::U32_LE,
            SampleFormat::F32 => AudioSampleFormat::F32_LE,
            SampleFormat::F64 => AudioSampleFormat::F64_LE,
            _ => return Err(BackendSpecificError {
                description: "Unsupported sample format".to_string(),
            })};
        Ok(value)
    }
}

impl TryFrom<AudioSampleFormat> for SampleFormat {
    type Error = BackendSpecificError;

    fn try_from(value: AudioSampleFormat) -> Result<Self, Self::Error> {
        let value = match value {
            AudioSampleFormat::S8 => SampleFormat::I8,
            AudioSampleFormat::U8 => SampleFormat::U8,
            AudioSampleFormat::S16_LE => SampleFormat::I16,
            AudioSampleFormat::U16_LE => SampleFormat::U16,
            AudioSampleFormat::S32_LE => SampleFormat::I32,
            AudioSampleFormat::U32_LE => SampleFormat::U32,
            AudioSampleFormat::F32_LE => SampleFormat::F32,
            AudioSampleFormat::F64_LE => SampleFormat::F64,
            _ => return Err(BackendSpecificError {
                description: "Unsupported sample format".to_string(),
            })};
            Ok(value)
    }
}

impl TryFrom<AudioSampleFormatEnum> for SampleFormat {
    type Error = BackendSpecificError;

    fn try_from(value: AudioSampleFormatEnum) -> Result<Self, Self::Error> {
        let sample_format = SampleFormat::try_from(value.default);
        if sample_format.is_ok() {
            return sample_format;
        }
        let sample_format = value.alternatives.iter()
            .map(move |sample_format| {
                SampleFormat::try_from(sample_format.clone())
            })
            .filter(move |result| result.is_ok())
            .last();
        sample_format.unwrap()
    }
}

pub trait FromStreamConfigWithSampleFormat {
    fn from(value: (&StreamConfig, SampleFormat)) -> Self;
}

impl FromStreamConfigWithSampleFormat for AudioStreamInfo {
    fn from(value: (&StreamConfig, SampleFormat)) -> Self {
        Self {
            media_type: MediaType::Audio,
            media_subtype: MediaSubtype::Raw,
            sample_format: value.1.try_into().unwrap(),
            sample_rate: value.0.sample_rate.0,
            channels: value.0.channels as u32,
            position: AudioChannelPosition::default(),
        }
    }
}

pub(super) struct AudioBuffer<'a> {
    buffer: pipewire::buffer::Buffer<'a>,
    sample_format: SampleFormat,
    channels: ChannelCount,
}

impl <'a> AudioBuffer<'a> {
    pub fn from(
        buffer: pipewire::buffer::Buffer<'a>,
        sample_format: SampleFormat,
        channels: ChannelCount,
    ) -> Self {
        Self {
            buffer,
            sample_format,
            channels
        }
    }
    
    pub fn data(&mut self) -> Option<Data> {
        let datas = self.buffer.datas_mut();
        let data = &mut datas[0];

        let stride = self.sample_format.sample_size() * self.channels as usize;

        let data_info = if let Some(data) = data.data() {
            let len = data.len();
            let data = unsafe {
                Some(Data::from_parts(
                    data.as_mut_ptr() as *mut (),
                    data.len() / self.sample_format.sample_size(),
                    self.sample_format,
                ))
            };
            (data, len)
        }
        else {
            return None
        };
        let chunk = data.chunk_mut();
        *chunk.offset_mut() = 0;
        *chunk.stride_mut() = stride as i32;
        *chunk.size_mut() = data_info.1 as u32;
        data_info.0
    }
}