#![feature(unsafe_destructor)]

#[cfg(all(not(windows)))]
use this_platform_is_not_supported;

#[cfg(windows)]
#[path="wasapi/mod.rs"]
pub mod cpal_impl;

pub struct Channel(cpal_impl::Channel);

pub struct Buffer<'a, T>(cpal_impl::Buffer<'a, T>);

impl Channel {
    pub fn new() -> Channel {
        let channel = cpal_impl::Channel::new();
        Channel(channel)
    }

    pub fn get_channels(&self) -> u16 {
        self.0.get_channels()
    }

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
