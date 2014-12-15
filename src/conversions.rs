/*!
This module contains function that will convert from one PCM format to another.

This includes conversion between samples formats, channels or sample rates.

*/
pub fn convert_samples_rate<T>(input: &[T], from: ::SamplesRate, to: ::SamplesRate) -> Vec<T>
                               where T: Copy
{
    unimplemented!()
}

pub fn convert_channels<T>(input: &[T], from: ::ChannelsCount, to: ::ChannelsCount) -> Vec<T>
                           where T: Copy
{
    assert!(input.len() % from as uint == 0);

    let mut result = Vec::new();

    for element in input.chunks(from as uint) {
        // copying the common channels
        for i in range(0, ::std::cmp::min(from, to)) {
            result.push(element[i as uint]);
        }

        // adding extra ones
        for i in range(0, ::std::cmp::max(0, to - from)) {
            result.push(element[i as uint % element.len()]);
        }
    }

    result
}

#[cfg(test)]
mod test {
    #[test]
    fn test_convert_channels() {

    }
}
