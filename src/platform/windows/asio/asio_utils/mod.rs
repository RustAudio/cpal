#[cfg(test)]
mod tests;

use std::marker::Copy;

/// Interleave the buffer from asio to cpal
/// asio: LLLLRRRR
/// cpal: LRLRLRLR
/// More then stereo:
/// asio: 111122223333
/// cpal: 123123123123
/// cpal buffer must have a length of exactly sum( all asio channel lengths )
/// this check is ommited for performance
pub fn interleave<T>(channels: &[Vec<T>], target: &mut Vec<T>)
where
    T: Copy,
{
    assert!(!channels.is_empty());
    target.clear();
    let frames = channels[0].len();
    target.extend((0 .. frames).flat_map(|f| channels.iter().map(move |ch| ch[f])));
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
    T: Copy,
{
    for ch in asio_channels.iter_mut() {
        ch.clear();
    }
    let num_channels = asio_channels.len();
    let mut ch = (0 .. num_channels).cycle();
    for &sample in cpal_buffer.iter() {
        asio_channels[ch.next().unwrap()].push(sample);
    }
}