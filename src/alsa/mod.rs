extern crate alsa_sys as alsa;
extern crate libc;

pub use self::enumerate::{EndpointsIterator, get_default_endpoint};

use ChannelPosition;
use CreationError;
use Format;
use FormatsEnumerationError;
use SampleFormat;
use SamplesRate;

use std::{ffi, iter, mem, ptr};
use std::vec::IntoIter as VecIntoIter;
use std::sync::Mutex;

pub type SupportedFormatsIterator = VecIntoIter<Format>;

mod enumerate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    pub fn get_supported_formats_list(&self)
            -> Result<SupportedFormatsIterator, FormatsEnumerationError>
    {
        unsafe {
            let mut playback_handle = mem::uninitialized();
            check_errors(alsa::snd_pcm_open(&mut playback_handle, b"hw\0".as_ptr() as *const _,
                                            alsa::SND_PCM_STREAM_PLAYBACK,
                                            alsa::SND_PCM_NONBLOCK)).unwrap();

            let hw_params = HwParams::alloc();
            check_errors(alsa::snd_pcm_hw_params_any(playback_handle, hw_params.0)).unwrap();

            // TODO: check endianess
            const FORMATS: [(SampleFormat, alsa::snd_pcm_format_t); 3] = [
                //SND_PCM_FORMAT_S8,
                //SND_PCM_FORMAT_U8,
                (SampleFormat::I16, alsa::SND_PCM_FORMAT_S16_LE),
                //SND_PCM_FORMAT_S16_BE,
                (SampleFormat::U16, alsa::SND_PCM_FORMAT_U16_LE),
                //SND_PCM_FORMAT_U16_BE,
                /*SND_PCM_FORMAT_S24_LE,
                SND_PCM_FORMAT_S24_BE,
                SND_PCM_FORMAT_U24_LE,
                SND_PCM_FORMAT_U24_BE,
                SND_PCM_FORMAT_S32_LE,
                SND_PCM_FORMAT_S32_BE,
                SND_PCM_FORMAT_U32_LE,
                SND_PCM_FORMAT_U32_BE,*/
                (SampleFormat::F32, alsa::SND_PCM_FORMAT_FLOAT_LE),
                /*SND_PCM_FORMAT_FLOAT_BE,
                SND_PCM_FORMAT_FLOAT64_LE,
                SND_PCM_FORMAT_FLOAT64_BE,
                SND_PCM_FORMAT_IEC958_SUBFRAME_LE,
                SND_PCM_FORMAT_IEC958_SUBFRAME_BE,
                SND_PCM_FORMAT_MU_LAW,
                SND_PCM_FORMAT_A_LAW,
                SND_PCM_FORMAT_IMA_ADPCM,
                SND_PCM_FORMAT_MPEG,
                SND_PCM_FORMAT_GSM,
                SND_PCM_FORMAT_SPECIAL,
                SND_PCM_FORMAT_S24_3LE,
                SND_PCM_FORMAT_S24_3BE,
                SND_PCM_FORMAT_U24_3LE,
                SND_PCM_FORMAT_U24_3BE,
                SND_PCM_FORMAT_S20_3LE,
                SND_PCM_FORMAT_S20_3BE,
                SND_PCM_FORMAT_U20_3LE,
                SND_PCM_FORMAT_U20_3BE,
                SND_PCM_FORMAT_S18_3LE,
                SND_PCM_FORMAT_S18_3BE,
                SND_PCM_FORMAT_U18_3LE,
                SND_PCM_FORMAT_U18_3BE,*/
            ];

            let mut supported_formats = Vec::new();
            for &(sample_format, alsa_format) in FORMATS.iter() {
                if alsa::snd_pcm_hw_params_test_format(playback_handle, hw_params.0, alsa_format) == 0 {
                    supported_formats.push(sample_format);
                }
            }

            let mut min_rate = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_rate_min(hw_params.0, &mut min_rate, ptr::null_mut())).unwrap();
            let mut max_rate = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_rate_max(hw_params.0, &mut max_rate, ptr::null_mut())).unwrap();

            let samples_rates = if min_rate == max_rate {
                vec![min_rate]
            } else if alsa::snd_pcm_hw_params_test_rate(playback_handle, hw_params.0, min_rate + 1, 0) == 0 {
                (min_rate .. max_rate + 1).collect()
            } else {
                const RATES: [libc::c_uint; 13] = [
                    5512,
                    8000,
                    11025,
                    16000,
                    22050,
                    32000,
                    44100,
                    48000,
                    64000,
                    88200,
                    96000,
                    176400,
                    192000,
                ];

                let mut rates = Vec::new();                
                for &rate in RATES.iter() {
                    if alsa::snd_pcm_hw_params_test_rate(playback_handle, hw_params.0, rate, 0) == 0 {
                        rates.push(rate);
                    }
                }

                if rates.len() == 0 {
                    (min_rate .. max_rate + 1).collect()
                } else {
                    rates
                }
            };

            let mut min_channels = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_channels_min(hw_params.0, &mut min_channels)).unwrap();
            let mut max_channels = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_get_channels_max(hw_params.0, &mut max_channels)).unwrap();
            let supported_channels = (min_channels .. max_channels + 1).filter_map(|num| {
                if alsa::snd_pcm_hw_params_test_channels(playback_handle, hw_params.0, num) == 0 {
                    Some(iter::repeat(ChannelPosition::FrontLeft).take(num as usize).collect::<Vec<_>>())        // FIXME: 
                } else {
                    None
                }
            }).collect::<Vec<_>>();

            let mut output = Vec::with_capacity(supported_formats.len() * supported_channels.len() *
                                                samples_rates.len());
            for &data_type in supported_formats.iter() {
                for channels in supported_channels.iter() {
                    for &rate in samples_rates.iter() {
                        output.push(Format {
                            channels: channels.clone(),
                            samples_rate: SamplesRate(rate as u32),
                            data_type: data_type,
                        });
                    }
                }
            }

            // TODO: RAII
            alsa::snd_pcm_close(playback_handle);
            Ok(output.into_iter())
        }
    }

    #[inline]
    pub fn get_name(&self) -> String {
        "unknown".to_owned()        // TODO: 
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

/// Wrapper around `hw_params`.
struct HwParams(*mut alsa::snd_pcm_hw_params_t);

impl HwParams {
    pub fn alloc() -> HwParams {
        unsafe {
            let mut hw_params = mem::uninitialized();
            check_errors(alsa::snd_pcm_hw_params_malloc(&mut hw_params)).unwrap();
            HwParams(hw_params)
        }
    }
}

impl Drop for HwParams {
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_hw_params_free(self.0);
        }
    }
}

impl Voice {
    pub fn new(endpoint: &Endpoint, format: &Format) -> Result<Voice, CreationError> {
        unsafe {
            let name = ffi::CString::new(endpoint.0.clone()).unwrap();

            let mut playback_handle = mem::uninitialized();
            check_errors(alsa::snd_pcm_open(&mut playback_handle, name.as_ptr(),
                                            alsa::SND_PCM_STREAM_PLAYBACK,
                                            alsa::SND_PCM_NONBLOCK)).unwrap();

            // TODO: check endianess
            let data_type = match format.data_type {
                SampleFormat::I16 => alsa::SND_PCM_FORMAT_S16_LE,
                SampleFormat::U16 => alsa::SND_PCM_FORMAT_U16_LE,
                SampleFormat::F32 => alsa::SND_PCM_FORMAT_FLOAT_LE,
            };

            let hw_params = HwParams::alloc();
            check_errors(alsa::snd_pcm_hw_params_any(playback_handle, hw_params.0)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_set_access(playback_handle, hw_params.0, alsa::SND_PCM_ACCESS_RW_INTERLEAVED)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_set_format(playback_handle, hw_params.0, data_type)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_set_rate(playback_handle, hw_params.0, format.samples_rate.0 as libc::c_uint, 0)).unwrap();
            check_errors(alsa::snd_pcm_hw_params_set_channels(playback_handle, hw_params.0, format.channels.len() as libc::c_uint)).unwrap();
            check_errors(alsa::snd_pcm_hw_params(playback_handle, hw_params.0)).unwrap();
            

            check_errors(alsa::snd_pcm_prepare(playback_handle)).unwrap();

            Ok(Voice {
                channel: Mutex::new(playback_handle),
                num_channels: format.channels.len() as u16,
            })
        }
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

    #[inline]
    pub fn play(&mut self) {
        // already playing
        //unimplemented!()
    }

    #[inline]
    pub fn pause(&mut self) {
        unimplemented!()
    }

    pub fn underflowed(&self) -> bool {
        false       // TODO: 
    }
}

unsafe impl Send for Voice {}
unsafe impl Sync for Voice {}

impl Drop for Voice {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            alsa::snd_pcm_close(*self.channel.lock().unwrap());
        }
    }
}

impl<'a, T> Buffer<'a, T> {
    #[inline]
    pub fn get_buffer<'b>(&'b mut self) -> &'b mut [T] {
        &mut self.buffer
    }

    #[inline]
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

#[inline]
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
