/*!
This module contains function that will convert from one PCM format to another.

This includes conversion between samples formats, channels or sample rates.

*/
use samples_formats::Sample;

/// Converts between samples rates while preserving the pitch.
pub fn convert_samples_rate<T>(input: &[T], from: ::SamplesRate, to: ::SamplesRate,
                               channels: ::ChannelsCount) -> Vec<T>
                               where T: Sample
{
    let from = from.0;
    let to = to.0;

    // if `from` is a multiple of `to` (for example `from` is 44100 and `to` is 22050),
    // then we simply skip some samples
    if from % to == 0 {
        let mut result = Vec::new();
        for element in input.chunks(channels as uint * (from / to) as uint) {
            for i in range(0, channels) {
                result.push(element[i as uint]);
            }
        }
        return result;
    }

    // if `to` is twice `from` (for example `to` is 44100 and `from` is 22050)
    // TODO: more generic
    if to == from * 2 {
        let mut result = Vec::new();
        let mut previous: Option<Vec<T>> = None;
        for element in input.chunks(channels as uint) {
            if let Some(previous) = previous.take() {
                for (prev, curr) in previous.into_iter().zip(element.iter()) {
                    result.push(prev.interpolate(*curr));
                }
                for curr in element.iter() {
                    result.push(*curr);
                }
            } else {
                for e in element.iter() {
                    result.push(*e);
                }
            }

            previous = Some(element.to_vec());
        }
        return result;
    }

    unimplemented!()
}

/// Converts between a certain number of channels.
///
/// If the target number is inferior to the source number, additional channels are removed.
///
/// If the target number is superior to the source number, the value of channel `N` is equal
/// to the value of channel `N % source_channels`.
///
/// ## Panic
///
/// Panics if `from` is 0, `to` is 0, or if the data length is not a multiple of `from`.
pub fn convert_channels<T>(input: &[T], from: ::ChannelsCount, to: ::ChannelsCount) -> Vec<T>
                           where T: Sample
{
    assert!(from != 0);
    assert!(to != 0);
    assert!(input.len() % from as uint == 0);

    let mut result = Vec::new();

    for element in input.chunks(from as uint) {
        // copying the common channels
        for i in range(0, ::std::cmp::min(from, to)) {
            result.push(element[i as uint]);
        }

        // adding extra ones
        if to > from {
            for i in range(0, to - from) {
                result.push(element[i as uint % element.len()]);
            }
        }
    }

    result
}

#[cfg(test)]
mod test {
    use super::convert_channels;
    use super::convert_samples_rate;

    #[test]
    fn remove_channels() {
        let result = convert_channels(&[1u16, 2, 3, 1, 2, 3], 3, 2);
        assert_eq!(result.as_slice(), [1, 2, 1, 2]);

        let result = convert_channels(&[1u16, 2, 3, 4, 1, 2, 3, 4], 4, 1);
        assert_eq!(result.as_slice(), [1, 1]);
    }

    #[test]
    fn add_channels() {
        let result = convert_channels(&[1u16, 2, 1, 2], 2, 3);
        assert_eq!(result.as_slice(), [1, 2, 1, 1, 2, 1]);

        let result = convert_channels(&[1u16, 2, 1, 2], 2, 4);
        assert_eq!(result.as_slice(), [1, 2, 1, 2, 1, 2, 1, 2]);
    }

    #[test]
    #[should_fail]
    fn convert_channels_wrong_data_len() {
        convert_channels(&[1u16, 2, 3], 2, 1);
    }

    #[test]
    fn half_samples_rate() {
        let result = convert_samples_rate(&[1u16, 16, 2, 17, 3, 18, 4, 19],
                                          ::SamplesRate(44100), ::SamplesRate(22050), 2);

        assert_eq!(result.as_slice(), [1, 16, 3, 18]);
    }

    #[test]
    fn double_samples_rate() {
        let result = convert_samples_rate(&[2u16, 16, 4, 18, 6, 20, 8, 22],
                                          ::SamplesRate(22050), ::SamplesRate(44100), 2);

        assert_eq!(result.as_slice(), [2, 16, 3, 17, 4, 18, 5, 19, 6, 20, 7, 21, 8, 22]);
    }
}
