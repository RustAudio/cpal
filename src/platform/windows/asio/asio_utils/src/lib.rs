use std::cell::RefCell;
use std::iter::Cloned;
use std::slice::{Iter, IterMut};

#[cfg(test)]
mod tests;

/// Interleave the buffer from asio to cpal
/// asio: LLLLRRRR
/// cpal: LRLRLRLR
/// More then stereo:
/// asio: 111122223333
/// cpal: 123123123123
/// cpal buffer must have a length of exactly sum( all asio channel lengths )
/// this check is ommited for performance
pub fn interleave<T>(channel_buffer: &[Vec<T>], cpal_buffer: &mut [T])
where
    T: std::marker::Copy,
{
    // TODO avoid this heap allocation
    // But we don't know how many channels we need.
    // Could use arrayvec if we make an upper limit
    let channels: Vec<RefCell<Cloned<Iter<T>>>> = channel_buffer
        .iter()
        .map(|c| RefCell::new(c.iter().cloned()))
        .collect();

    for (c_buff, channel) in cpal_buffer.iter_mut().zip(channels.iter().cycle()) {
        match channel.borrow_mut().next() {
            Some(c) => *c_buff = c,
            None => break,
        }
    }
}

/// Function for deinterleaving because
/// cpal writes to buffer interleaved
/// cpal: LRLRLRLR
/// asio: LLLLRRRR
/// More then stereo:
/// cpal: 123123123123
/// asio: 111122223333
pub fn deinterleave<T>(cpal_buffer: &[T], asio_channels: &mut [Vec<T>])
where
    T: std::marker::Copy,
{
    // TODO avoid this heap allocation
    // Possibly use arrayvec and some max channels
    let channels: Vec<RefCell<IterMut<T>>> = asio_channels
        .iter_mut()
        .map(|c| RefCell::new(c.iter_mut()))
        .collect();

    for (c_buff, a_channel) in cpal_buffer.iter().zip(channels.iter().cycle()) {
        match a_channel.borrow_mut().next() {
            Some(c) => *c = *c_buff,
            None => break,
        }
    }
}
