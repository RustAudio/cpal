/*!
# How to use cpal

In order to play a sound, first you need to create a `Voice`.

```no_run
let mut voice = cpal::Voice::new();
```

Then you must send raw samples to it by calling `append_data`.
This function takes three parameters: the number of channels, the number of samples
that must be played per second, and the number of samples that you have available.

You can then fill the buffer with the data.

```no_run
# let mut voice = cpal::Voice::new();
let mut buffer: cpal::Buffer<f32> = voice.append_data(2, cpal::SamplesRate(44100), 1024);

// filling the buffer with 0s
for e in buffer.iter_mut() {
    *e = 0.0f32;
}
```

**Important**: the `append_data` function can return a buffer shorter than what you requested.
This is the case if the device doesn't have enough space available. **It happens very often**,
this is not some obscure situation that can be ignored.

After you have submitted data for the first time, call `play`:

```no_run
# let mut voice = cpal::Voice::new();
voice.play();
```

The audio device of the user will read the buffer that you sent, and play it. If the audio device
reaches the end of the data, it will stop playing. You must continuously fill the buffer by
calling `append_data` repeatedly if you don't want the audio to stop playing.

# Native format

Each `Voice` is bound to a specific number of channels, samples rate, and samples format.
If you call `append_data` with values different than these, then cpal will automatically perform
a conversion on your data.

If you have the possibility, you should try to match the format of the voice.

*/
#![feature(box_syntax, core, unsafe_destructor, thread_sleep, std_misc)]

pub use samples_formats::{SampleFormat, Sample};

use std::ops::{Deref, DerefMut};

mod conversions;
mod samples_formats;

#[cfg(target_os = "linux")]
#[path="alsa/mod.rs"]
mod cpal_impl;

#[cfg(windows)]
#[path="wasapi/mod.rs"]
mod cpal_impl;

#[cfg(target_os = "macos")]
#[path="coreaudio/mod.rs"]
mod cpal_impl;

#[cfg(all(not(windows), not(unix)))]
#[path="null/mod.rs"]
mod cpal_impl;

/// Controls a sound output. A typical application has one `Voice` for each sound
/// it wants to output.
///
/// A voice must be periodically filled with new data by calling `append_data`, or the sound
/// will stop playing.
///
/// Each `Voice` is bound to a specific number of channels, samples rate, and samples format,
/// which can be retreived by calling `get_channels`, `get_samples_rate` and `get_samples_format`.
/// If you call `append_data` with values different than these, then cpal will automatically
/// perform a conversion on your data.
///
/// If you have the possibility, you should try to match the format of the voice.
pub struct Voice(cpal_impl::Voice);

/// Number of channels.
pub type ChannelsCount = u16;

/// 
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SamplesRate(pub u32);

/// Represents a buffer that must be filled with audio data.
///
/// You should destroy this object as soon as possible. Data is only committed when it
/// is destroyed.
#[must_use]
pub struct Buffer<'a, T: 'a> where T: Sample {
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
    to_format: SampleFormat,
    from_channels: ChannelsCount,
    to_channels: ChannelsCount,
}

impl Voice {
    /// Builds a new channel.
    pub fn new() -> Voice {
        let channel = cpal_impl::Voice::new();
        Voice(channel)
    }

    /// Returns the number of channels.
    ///
    /// You can add data with any number of channels, but matching the voice's native format
    /// will lead to better performances.
    pub fn get_channels(&self) -> ChannelsCount {
        self.0.get_channels()
    }

    /// Returns the number of samples that are played per second.
    ///
    /// You can add data with any samples rate, but matching the voice's native format
    /// will lead to better performances.
    pub fn get_samples_rate(&self) -> SamplesRate {
        self.0.get_samples_rate()
    }

    /// Returns the format of the samples that are accepted by the backend.
    ///
    /// You can add data of any format, but matching the voice's native format
    /// will lead to better performances.
    pub fn get_samples_format(&self) -> SampleFormat {
        self.0.get_samples_format()
    }

    /// Adds some PCM data to the voice's buffer.
    ///
    /// This function returns a `Buffer` object that must be filled with the audio data.
    /// The size of the buffer being returned depends on the current state of the backend
    /// and can't be known in advance. However it is never greater than `max_elements`.
    ///
    /// You must fill the buffer *entirely*, so do not set `max_elements` to a value greater
    /// than the amount of data available to you.
    ///
    /// Channels are interleaved. For example if you have two channels, you must write
    /// the first sample of the first channel, then the first sample of the second channel,
    /// then the second sample of the first channel, then the second sample of the second
    /// channel, etc.
    ///
    /// ## Parameters
    ///
    /// * `channels`: number of channels (1 for mono, 2 for stereo, etc.)
    /// * `samples_rate`: number of samples that must be played by second for each channel
    /// * `max_elements`: maximum size of the returned buffer
    ///
    /// ## Panic
    ///
    /// Panics if `max_elements` is 0 or is not a multiple of `channels`.
    ///
    pub fn append_data<'a, T>(&'a mut self, channels: ChannelsCount,
                              samples_rate: SamplesRate, max_elements: usize)
                              -> Buffer<'a, T> where T: Sample + Clone
    {
        assert!(max_elements != 0);
        assert!(max_elements % channels as usize == 0);

        let target_samples_rate = self.0.get_samples_rate();
        let target_channels = self.0.get_channels();

        let source_samples_format = Sample::get_format(None::<T>);
        let target_samples_format = self.0.get_samples_format();

        // if we need to convert the incoming data
        if samples_rate != target_samples_rate || channels != target_channels ||
           source_samples_format != target_samples_format
        {
            let max_elements = max_elements * target_channels as usize / channels as usize;
            let max_elements = max_elements * target_samples_rate.0 as usize /
                               samples_rate.0 as usize;

            let mut target_buffer = self.0.append_data(max_elements);

            // computing the length of the intermediary buffer
            let intermediate_buffer_length = target_buffer.get_buffer().len();
            let intermediate_buffer_length = intermediate_buffer_length * channels as usize /
                                             target_channels as usize;
            let intermediate_buffer_length = intermediate_buffer_length * samples_rate.0 as usize /
                                             target_samples_rate.0 as usize;
            let intermediate_buffer = std::iter::repeat(unsafe { std::mem::uninitialized() })
                                        .take(intermediate_buffer_length).collect();

            Buffer {
                target: Some(target_buffer),
                conversion: Some(RequiredConversion {
                    intermediate_buffer: intermediate_buffer,
                    from_sample_rate: samples_rate,
                    to_sample_rate: target_samples_rate,
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

    /// Sends a command to the audio device that it should start playing.
    ///
    /// Has no effect is the voice was already playing.
    ///
    /// Only call this after you have submitted some data, otherwise you may hear
    /// some glitches.
    pub fn play(&mut self) {
        self.0.play()
    }

    /// Sends a command to the audio device that it should stop playing.
    ///
    /// Has no effect is the voice was already paused.
    ///
    /// If you call `play` afterwards, the playback will resume exactly where it was.
    pub fn pause(&mut self) {
        self.0.pause()
    }
}

impl<'a, T> Deref for Buffer<'a, T> where T: Sample {
    type Target = [T];

    fn deref(&self) -> &[T] {
        panic!("It is forbidden to read from the audio buffer");
    }
}

impl<'a, T> DerefMut for Buffer<'a, T> where T: Sample {
    fn deref_mut(&mut self) -> &mut [T] {
        if let Some(ref mut conversion) = self.conversion {
            &mut conversion.intermediate_buffer
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
                conversions::convert_channels(&buffer, conversion.from_channels,
                                              conversion.to_channels)
            } else {
                buffer
            };

            let buffer = if conversion.from_sample_rate != conversion.to_sample_rate {
                conversions::convert_samples_rate(&buffer, conversion.from_sample_rate,
                                                  conversion.to_sample_rate,
                                                  conversion.to_channels)
            } else {
                buffer
            };

            let output = self.target.as_mut().unwrap().get_buffer();
            assert!(buffer.len() == output.len(), "Buffers length mismatch: {} vs {}", buffer.len(), output.len());

            macro_rules! write_to_buf(
                ($buf:expr, $output:expr, $ty:ty) => ({
                    use std::borrow::Cow;

                    let output: &mut [$ty] = unsafe { std::mem::transmute($output) };

                    match $buf {
                        Cow::Borrowed(buf) => {
                            for (i, o) in buf.iter().zip(output.iter_mut()) {
                                *o = *i;
                            }
                        },
                        Cow::Owned(buf) => {
                            for (i, o) in buf.into_iter().zip(output.iter_mut()) {
                                *o = i;
                            }
                        }
                    }
                })
            );

            match conversion.to_format {
                SampleFormat::I16 => {
                    let buffer = Sample::to_vec_i16(&buffer);
                    write_to_buf!(buffer, output, i16);
                },
                SampleFormat::U16 => {
                    let buffer = Sample::to_vec_u16(&buffer);
                    write_to_buf!(buffer, output, u16);
                },
                SampleFormat::F32 => {
                    let buffer = Sample::to_vec_f32(&buffer);
                    write_to_buf!(buffer, output, f32);
                },
            }
        }

        self.target.take().unwrap().finish();
    }
}
