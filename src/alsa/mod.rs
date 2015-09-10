extern crate alsa_sys as alsa;
extern crate libc;

pub use self::enumerate::{EndpointsIterator, get_default_endpoint};

use ChannelPosition;
use CreationError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SamplesRate;

use std::{ffi, iter, mem};
use std::option::IntoIter as OptionIntoIter;
use std::sync::Mutex;

pub type SupportedFormatsIterator = OptionIntoIter<Format>;

mod enumerate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        let format = Format {
            channels: vec![ChannelPosition::FrontLeft, ChannelPosition::FrontRight],
            samples_rate: SamplesRate(44100),
            data_type: SampleFormat::I16,
        };

        Ok(Some(format).into_iter())
    }
}

pub struct Voice {
    channel: Mutex<*mut alsa::snd_pcm_t>,
    num_channels: u16,
}

pub struct Buffer<'a, T> {
    channel: &'a mut Voice,
    buffer: Vec<T>,
}

impl Voice {
    pub fn new(endpoint: &Endpoint, _format: &Format) -> Result<Voice, CreationError> {
        unsafe {
            let name = ffi::CString::new(endpoint.0.clone()).unwrap();

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

            Ok(Voice {
                channel: Mutex::new(playback_handle),
                num_channels: 2,
            })
        }
    }

    pub fn get_channels(&self) -> ::ChannelsCount {
        self.num_channels
    }

    pub fn get_samples_rate(&self) -> ::SamplesRate {
        ::SamplesRate(44100)
    }

    pub fn get_samples_format(&self) -> ::SampleFormat {
        ::SampleFormat::I16
    }

    pub fn append_data<'a, T>(&'a mut self, max_elements: usize) -> Buffer<'a, T> where T: Clone {
        let available = {
            let channel = self.channel.lock().unwrap();
            let available = unsafe { alsa::snd_pcm_avail(*channel) };
            available * self.num_channels as alsa::snd_pcm_sframes_t
        };

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
            alsa::snd_pcm_close(*self.channel.lock().unwrap());
        }
    }
}

impl<'a, T> Buffer<'a, T> {
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        &mut self.buffer
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn finish(self) {
        let written = (self.buffer.len() / self.channel.num_channels as usize)
                      as alsa::snd_pcm_uframes_t;
        let channel = self.channel.channel.lock().unwrap();

        unsafe {
            let result = alsa::snd_pcm_writei(*channel,
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
            let s = ffi::CStr::from_ptr(alsa::snd_strerror(err)).to_bytes().to_vec();
            let s = String::from_utf8(s).unwrap();
            return Err(s);
        }
    }

    Ok(())
}
