#![feature(unsafe_destructor)]

#[cfg(all(not(windows), not(unix)))]
use this_platform_is_not_supported;

mod conversions;

#[cfg(unix)]
#[path="alsa/mod.rs"]
pub mod cpal_impl;
#[cfg(windows)]
#[path="wasapi/mod.rs"]
pub mod cpal_impl;

/// A `Channel` represents a sound output.
///
/// A channel must be periodically filled with new data, or the sound will
/// stop playing.
pub struct Channel(cpal_impl::Channel);

/// Number of channels.
pub type ChannelsCount = u16;

/// 
#[deriving(Show, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SamplesRate(pub u32);

/// Represents a buffer that must be filled with audio data.
///
/// A `Buffer` object borrows the channel.
pub struct Buffer<'a, T> {
    // also contains something, taken by `Drop`
    target: Option<cpal_impl::Buffer<'a, T>>, 
    // if this is non-none, then the data will be written to `conversion.intermediate_buffer`
    // instead of `target`, and the conversion will be done in buffer's destructor
    conversion: Option<RequiredConversion<T>>,
}

struct RequiredConversion<T> {
    intermediate_buffer: Vec<T>,
    from_sample_rate: SamplesRate,
    to_sample_rate: SamplesRate,
    from_format: SampleFormat,
    to_format: SampleFormat,
    from_channels: ChannelsCount,
    to_channels: ChannelsCount,
}

/// Format that each sample has.
#[deriving(Clone, Copy, Show, PartialEq, Eq)]
pub enum SampleFormat {
    /// The value 0 corresponds to 0.
    I16,
    /// The value 0 corresponds to 32768.
    U16,
}

/// Trait for containers that contain PCM data.
#[unstable = "Will be rewritten with associated types"]
pub trait Sample: Copy {
    fn get_format(Option<Self>) -> SampleFormat;

    /// Turns the data into a `Vec<u16>` where each element is a sample.
    fn to_vec_u16(&[Self]) -> Vec<u16>;
}

impl Sample for u16 {
    fn get_format(_: Option<u16>) -> SampleFormat {
        SampleFormat::U16
    }

    fn to_vec_u16(input: &[u16]) -> Vec<u16> {
        input.to_vec()
    }
}

impl Channel {
    pub fn new() -> Channel {
        let channel = cpal_impl::Channel::new();
        Channel(channel)
    }

    /// Returns the number of channels.
    ///
    /// 1 for mono, 2 for stereo, etc.
    pub fn get_channels(&self) -> ChannelsCount {
        self.0.get_channels()
    }

    /// Returns the number of samples that are played per second.
    ///
    /// Common values are 22050 Hz or 44100 Hz.
    pub fn get_samples_rate(&self) -> SamplesRate {
        self.0.get_samples_rate()
    }

    /// Returns the number of samples that are played per second.
    ///
    /// Common values are 22050 Hz or 44100 Hz.
    pub fn get_samples_format(&self) -> SampleFormat {
        self.0.get_samples_format()
    }

    /// Adds some PCM data to the channel's buffer.
    ///
    /// This function returns a `Buffer` object that must be filled with the audio data.
    /// You can't know in advance the size of the buffer, as it depends on the current state
    /// of the backend.
    ///
    /// ## Panic
    ///
    /// Panics if `max_elements` is 0 or is not a multiple of `channels`.
    ///
    pub fn append_data<'a, T>(&'a mut self, channels: ChannelsCount,
                              samples_rate: SamplesRate, max_elements: uint)
                              -> Buffer<'a, T> where T: Sample + Clone
    {
        assert!(max_elements != 0);
        assert!(max_elements % channels as uint == 0);

        let target_samples_rate = self.0.get_samples_rate();
        let target_channels = self.0.get_channels();

        let source_samples_format = Sample::get_format(None::<T>);
        let target_samples_format = self.0.get_samples_format();

        // if we need to convert the incoming data
        if samples_rate != target_samples_rate || channels != target_channels ||
           source_samples_format != target_samples_format
        {
            let max_elements = max_elements * target_channels as uint / channels as uint;
            let max_elements = max_elements * target_samples_rate.0 as uint / samples_rate.0 as uint;
            // TODO: samples format

            let mut target_buffer = self.0.append_data(max_elements);

            // computing the length of the intermediary buffer
            let intermediate_buffer_length = target_buffer.get_buffer().len();
            let intermediate_buffer_length = intermediate_buffer_length * channels as uint /
                                             target_channels as uint;
            let intermediate_buffer_length = intermediate_buffer_length * samples_rate.0 as uint /
                                             target_samples_rate.0 as uint;
            // TODO: adapt size to samples format too
            let intermediate_buffer = Vec::from_elem(intermediate_buffer_length, unsafe { std::mem::uninitialized() });

            Buffer {
                target: Some(target_buffer),
                conversion: Some(RequiredConversion {
                    intermediate_buffer: intermediate_buffer,
                    from_sample_rate: samples_rate,
                    to_sample_rate: target_samples_rate,
                    from_format: source_samples_format,
                    to_format: target_samples_format,
                    from_channels: channels,
                    to_channels: target_channels,
                }),
            }

        } else {
            Buffer {
                target: Some(self.0.append_data(max_elements)), 
                conversion: None,
            }
        }
    }
}

impl<'a, T> Deref<[T]> for Buffer<'a, T> {
    fn deref(&self) -> &[T] {
        panic!("It is forbidden to read from the audio buffer");
    }
}

impl<'a, T> DerefMut<[T]> for Buffer<'a, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        if let Some(ref mut conversion) = self.conversion {
            conversion.intermediate_buffer.as_mut_slice()
        } else {
            self.target.as_mut().unwrap().get_buffer()
        }
    }
}

#[unsafe_destructor]
impl<'a, T> Drop for Buffer<'a, T> where T: Sample {
    fn drop(&mut self) {
        if let Some(conversion) = self.conversion.take() {
            let buffer = conversion.intermediate_buffer;

            let buffer = if conversion.from_channels != conversion.to_channels {
                conversions::convert_channels(buffer.as_slice(), conversion.from_channels,
                                              conversion.to_channels)
            } else {
                buffer
            };

            let buffer = if conversion.from_sample_rate != conversion.to_sample_rate {
                conversions::convert_samples_rate(buffer.as_slice(), conversion.from_sample_rate,
                                                  conversion.to_sample_rate)
            } else {
                buffer
            };

            /*let buffer = if conversion.from_format != conversion.to_format {
                match conversion.to_format {
                    SampleFormat::U16 => Sample::to_vec_u16(buffer.as_slice()),
                    _ => unimplemented!(),
                }
            } else {
                buffer
            };*/
            if conversion.from_format != conversion.to_format { unimplemented!() }

            let output = self.target.as_mut().unwrap().get_buffer();
            assert!(buffer.len() == output.len(), "Buffers length mismatch: {} vs {}", buffer.len(), output.len());
            for (i, o) in buffer.into_iter().zip(output.iter_mut()) {
                *o = i;
            }
        }

        self.target.take().unwrap().finish();
    }
}
