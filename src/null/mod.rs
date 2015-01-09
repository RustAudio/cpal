pub struct Voice;
pub struct Buffer<'a, T>;

impl Voice {
    pub fn new() -> Voice {
        Voice
    }

    pub fn get_channels(&self) -> ::ChannelsCount {
        2
    }

    pub fn get_samples_rate(&self) -> ::SamplesRate {
        ::SamplesRate(44100)
    }

    pub fn get_samples_format(&self) -> ::SampleFormat {
        ::SampleFormat::U16
    }

    pub fn append_data<'a, T>(&'a mut self, _: uint) -> Buffer<'a, T> {
        Buffer
    }

    pub fn play(&mut self) {
    }

    pub fn pause(&mut self) {
    }
}

impl<'a, T> Buffer<'a, T> {
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        [].as_mut_slice()
    }

    pub fn finish(self) {
    }
}
