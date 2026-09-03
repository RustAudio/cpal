use std::{ptr, slice};
use azo::dto::ChannelId;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Config {
    pub format  : SampleFormat,
    pub channels: u16,
    pub input   : bool,
}

impl Config {
    pub fn validate(self, driver: &Driver) -> impl Iterator<Item=CpalResult<ChannelId>> {
        (0..self.channels)
            .map(move |i| {
                let id = ChannelId { input: self.input, index: i as _ };
                let actual_format = driver
                    .channel_info(id)
                    .map_err(|error| create_report(driver, error, "channel_info"))?
                    .sample_type
                    .pipe(sample_format_azo2cpal);
                if actual_format != Some(self.format) {
                    return err(UnsupportedConfig, "Sample format mismatch");
                }
                Ok(id)
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Head {
    pub format: SampleFormat,
    pub frame_count: FrameCount,
    pub buf_ptrs: Vec<DoubleBuffer>
}

impl Head {
    const fn frame_count(&self) -> usize {
        self.frame_count as usize
    }

    const fn channel_count(&self) -> usize {
        self.buf_ptrs.len()
    }

    fn sample_size(&self) -> usize {
        self.format.sample_size()
    }

    const fn sample_count(&self) -> usize {
        self.frame_count() * self.channel_count()
    }

    fn bytes_per_channel(&self) -> usize {
        self.frame_count() * self.sample_size()
    }

    fn _frame_size(&self) -> usize {
        self.channel_count() * self.sample_size()
    }

    fn total_buffer_space(&self) -> usize {
        self.frame_count() * self.channel_count() * self.sample_size()
    }

    fn get_buf_ptr(&self, channel: usize, dbuf_side: usize) -> *mut u8 {
        self.buf_ptrs[channel]
            .0[dbuf_side]
            .cast()
    }

    fn get_buf<'buf>(&self, channel: usize, dbuf_side: usize) -> &'buf [u8] {
        let ptr = self.get_buf_ptr(channel, dbuf_side);
        let len = self.bytes_per_channel();
        unsafe { slice::from_raw_parts(ptr, len) }
    }

    fn get_buf_mut<'buf>(&self, channel: usize, dbuf_side: usize) -> &'buf mut [u8] {
        let ptr = self.get_buf_ptr(channel, dbuf_side);
        let len = self.bytes_per_channel();
        unsafe { slice::from_raw_parts_mut(ptr, len) }
    }
}

pub struct WithScratch {
    head: Head,
    scratch: Box<[u8]>
}

impl WithScratch {
    pub fn new(format: SampleFormat, frame_count: FrameCount, buf_ptrs: Vec<DoubleBuffer>) -> Self {
        let head = Head { format, frame_count, buf_ptrs };
        
        // when the stream is mono, the ASIO buffer can be exposed to the user callback directly
        let scratch_len = if head.channel_count() == 1 { 0 } else { head.total_buffer_space() };
        let scratch = vec![0; scratch_len].into_boxed_slice();
        Self { head, scratch }
    }

    pub fn data(&mut self, dbuf_side: usize) -> Data {
        let ptr = match self.head.channel_count() {
            0   => ptr::null_mut(),
            1   => self.head.get_buf_ptr(0, dbuf_side).cast(),
            2.. => self.scratch.as_mut_ptr().cast()
        };
        unsafe {
            Data::from_parts(
                ptr,
                self.head.sample_count(),
                self.head.format
            )
        }
    }

    /// copies channel data to the scratch buffer, interleaving it in the process
    pub fn interleave(&mut self, dbuf_side: usize) {
        if self.head.channel_count() < 2 {
            return;
        }
        let stride = self.head.sample_size();
        let scratch_frames = self.scratch.chunks_exact_mut(self.head.channel_count() * stride);
        for (i_frame, scratch_frame) in scratch_frames.enumerate() {
            for (i_channel, scratch_sample) in scratch_frame.chunks_exact_mut(stride).enumerate() {
                let pos = i_frame * stride;
                scratch_sample.copy_from_slice(&self.head.get_buf(i_channel, dbuf_side)[pos..][..stride]);
            }
        }
    }
    /// copies scratch data to the channels, deinterleaving it in the process
    pub fn deinterleave(&self, dbuf_side: usize) {
        if self.head.channel_count() < 2 {
            return;
        }
        let stride = self.head.sample_size();
        let scratch_frames = self.scratch.chunks_exact(self.head.channel_count() * stride);
        for (i_frame, scratch_frame) in scratch_frames.enumerate() {
            for (i_channel, scratch_sample) in scratch_frame.chunks_exact(stride).enumerate() {
                let pos = i_frame * stride;
                self.head.get_buf_mut(i_channel, dbuf_side)[pos..][..stride].copy_from_slice(scratch_sample);
            }
        }
    }
}