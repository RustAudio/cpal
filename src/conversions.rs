/*!
This module contains function that will convert from one PCM format to another.

This includes conversion between samples formats, channels or sample rates.

*/
pub fn convert_samples_rate<T>(input: &[T], from: ::SamplesRate, to: ::SamplesRate) -> Vec<T>
                               where T: Copy
{
    let from = from.0;
    let to = to.0;

    // if `from` is a multiple of `to` (for example `from` is 44100 and `to` is 22050),
    // then we simply skip some samples
    if from % to  == 0 {
        let mut result = Vec::new();
        for element in input.chunks((from / to) as uint) {
            result.push(element[0]);
        }
        return result;
    }

    // if `to` is a multiple of `from` (for example `to` is 44100 and `from` is 22050)
    // TODO: dumb algorithm
    // FIXME: doesn't take channels into account
    if to % from  == 0 {
        let mut result = Vec::new();
        for element in input.windows(2) {
            for _ in range(0, (to / from) as uint) {
                result.push(element[0]);
            }
        }
        for _ in range(0, (to / from) as uint) {
            result.push(*input.last().unwrap());
        }
        return result;
    }

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

/*#[cfg(test)]
mod test {
    #[test]
    fn test_convert_channels() {

    }
}*/
