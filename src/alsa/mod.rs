extern crate "alsa-sys" as alsa;
extern crate libc;

use std::{ffi, iter, mem};

pub struct Voice {
    channel: *mut alsa::snd_pcm_t,
    num_channels: u16,
}

pub struct Buffer<'a, T> {
    channel: &'a mut Voice,
    buffer: Vec<T>,
}

impl Voice {
    pub fn new() -> Voice {
        unsafe {
            let name = ffi::CString::from_slice(b"default");

            let mut playback_handle = mem::uninitialized();
            check_errors(alsa::snd_pcm_open(&mut playback_handle, name.as_ptr(), alsa::SND_PCM_STREAM_PLAYBACK, alsa::SND_PCM_NONBLOCK)).unwrap();

            let mut hw_params = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_malloc(&mut hw_params)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_any(playback_handle, hw_params)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_set_access(playback_handle, hw_params, alsa::SND_PCM_ACCESS_RW_INTERLEAVED)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_set_format(playback_handle, hw_params, alsa::SND_PCM_FORMAT_S16_LE)).unwrap(); // TODO: check endianess
            check_errors(alsa::snd_pcm_hw_params_set_rate(playback_handle, hw_params, 44100, 0)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_set_channels(playback_handle, hw_params, 2)).unwrap();
            check_errors(alsa::snd_pcm_hw_params(playback_handle, hw_params)).unwrap();
            alsa::snd_pcm_hw_params_free(hw_params);

            check_errors(alsa::snd_pcm_prepare(playback_handle)).unwrap();

            Voice {
                channel: playback_handle,
                num_channels: 2,
            }
        }
    }

    pub fn get_channels(&self) -> ::ChannelsCount {
        self.num_channels
    }

    pub fn get_samples_rate(&self) -> ::SamplesRate {
        ::SamplesRate(44100)
    }

    pub fn get_samples_format(&self) -> ::SampleFormat {
        ::SampleFormat::U16
    }

    pub fn append_data<'a, T>(&'a mut self, max_elements: usize) -> Buffer<'a, T> where T: Clone {
        let available = unsafe { alsa::snd_pcm_avail(self.channel) };
        let available = available * self.num_channels as alsa::snd_pcm_sframes_t;

        let elements = ::std::cmp::min(available as usize, max_elements);

        Buffer {
            channel: self,
            buffer: iter::repeat(unsafe { mem::uninitialized() }).take(elements).collect(),
        }
    }

    pub fn play(&mut self) {
        // already playing
        //unimplemented!()
    }

    pub fn pause(&mut self) {
        unimplemented!()
    }
}

unsafe impl Send for Voice {}
unsafe impl Sync for Voice {}

impl Drop for Voice {
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_close(self.channel);
        }
    }
}

impl<'a, T> Buffer<'a, T> {
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        self.buffer.as_mut_slice()
    }

    pub fn finish(self) {
        let written = (self.buffer.len() / self.channel.num_channels as usize) as alsa::snd_pcm_uframes_t;

        unsafe {
            let result = alsa::snd_pcm_writei(self.channel.channel,
                                              self.buffer.as_ptr() as *const libc::c_void,
                                              written);

            if result < 0 {
                check_errors(result as libc::c_int).unwrap();
            }
        }
    }
}

fn check_errors(err: libc::c_int) -> Result<(), String> {
    use std::ffi;

    if err < 0 {
        unsafe {
            let s = String::from_utf8(ffi::c_str_to_bytes(&alsa::snd_strerror(err)).to_vec());
            return Err(s.unwrap());
        }
    }

    Ok(())
}
