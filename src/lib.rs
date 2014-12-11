#![feature(unsafe_destructor)]

#[cfg(all(not(windows)))]
use this_platform_is_not_supported;

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

/// Represents a buffer that must be filled with audio data.
///
/// A `Buffer` object borrows the channel.
pub struct Buffer<'a>(cpal_impl::Buffer<'a>);

/// Format that each sample has.
#[deriving(Clone, Show, PartialEq, Eq)]
pub enum SampleFormat {
    /// The value 0 corresponds to 0.
    I16,
    /// The value 0 corresponds to 32768.
    U16,
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
    pub fn get_samples_per_second(&self) -> u32 {
        self.0.get_samples_per_second()
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
    pub fn append_data<'a>(&'a mut self) -> Buffer<'a> {
        Buffer(self.0.append_data())
    }
}

impl<'a> Deref<[u8]> for Buffer<'a> {
    fn deref(&self) -> &[u8] {
        panic!()
    }
}

impl<'a> DerefMut<[u8]> for Buffer<'a> {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.0.get_buffer()
    }
}
