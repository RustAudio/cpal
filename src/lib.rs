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

/// Represents a buffer that must be filled with audio data.
///
/// A `Buffer` object borrows the channel.
pub struct Buffer<'a, T>(cpal_impl::Buffer<'a, T>);

impl Channel {
    pub fn new() -> Channel {
        let channel = cpal_impl::Channel::new();
        Channel(channel)
    }

    /// Returns the number of channels.
    ///
    /// 1 for mono, 2 for stereo, etc.
    pub fn get_channels(&self) -> u16 {
        self.0.get_channels()
    }

    /// Adds some PCM data to the channel's buffer.
    ///
    /// This function returns a `Buffer` object that must be filled with the audio data.
    /// You can't know in advance the size of the buffer, as it depends on the current state
    /// of the backend.
    pub fn append_data<'a, T>(&'a mut self) -> Buffer<'a, T> {
        Buffer(self.0.append_data())
    }
}

impl<'a, T> Deref<[T]> for Buffer<'a, T> {
    fn deref(&self) -> &[T] {
        panic!()
    }
}

impl<'a, T> DerefMut<[T]> for Buffer<'a, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.0.get_buffer()
    }
}
