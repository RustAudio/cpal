use std::marker::PhantomData;
use std::time::Instant;

extern crate oboe;

use super::convert::{stream_instant, to_stream_instant};
use crate::{Data, InputCallbackInfo, InputStreamTimestamp, SizedSample, StreamError};

pub struct CpalInputCallback<I, C> {
    data_cb: Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>,
    error_cb: Box<dyn FnMut(StreamError) + Send + 'static>,
    created: Instant,
    phantom_channel: PhantomData<C>,
    phantom_input: PhantomData<I>,
}

impl<I, C> CpalInputCallback<I, C> {
    pub fn new<D, E>(data_cb: D, error_cb: E) -> Self
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        Self {
            data_cb: Box::new(data_cb),
            error_cb: Box::new(error_cb),
            created: Instant::now(),
            phantom_channel: PhantomData,
            phantom_input: PhantomData,
        }
    }

    fn make_callback_info(
        &self,
        audio_stream: &mut dyn oboe::AudioInputStreamSafe,
    ) -> InputCallbackInfo {
        InputCallbackInfo {
            timestamp: InputStreamTimestamp {
                callback: to_stream_instant(self.created.elapsed()),
                capture: stream_instant(audio_stream),
            },
        }
    }
}

impl<T: SizedSample, C: oboe::IsChannelCount> oboe::AudioInputCallback for CpalInputCallback<T, C>
where
    (T, C): oboe::IsFrameType,
{
    type FrameType = (T, C);

    fn on_error_before_close(
        &mut self,
        _audio_stream: &mut dyn oboe::AudioInputStreamSafe,
        error: oboe::Error,
    ) {
        (self.error_cb)(StreamError::from(error))
    }

    fn on_error_after_close(
        &mut self,
        _audio_stream: &mut dyn oboe::AudioInputStreamSafe,
        error: oboe::Error,
    ) {
        (self.error_cb)(StreamError::from(error))
    }

    fn on_audio_ready(
        &mut self,
        audio_stream: &mut dyn oboe::AudioInputStreamSafe,
        audio_data: &[<<Self as oboe::AudioInputCallback>::FrameType as oboe::IsFrameType>::Type],
    ) -> oboe::DataCallbackResult {
        let cb_info = self.make_callback_info(audio_stream);
        let channel_count = if C::CHANNEL_COUNT == oboe::ChannelCount::Mono {
            1
        } else {
            2
        };
        (self.data_cb)(
            &unsafe {
                Data::from_parts(
                    audio_data.as_ptr() as *mut _,
                    audio_data.len() * channel_count,
                    T::FORMAT,
                )
            },
            &cb_info,
        );
        oboe::DataCallbackResult::Continue
    }
}
