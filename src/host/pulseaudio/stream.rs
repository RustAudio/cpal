use std::{
    sync::{
        atomic::{self, AtomicU64},
        Arc, Mutex,
    },
    time,
};

use futures::executor::block_on;
use pulseaudio::{protocol, AsPlaybackSource};

use crate::{
    traits::StreamTrait, BackendSpecificError, BuildStreamError, Data, InputCallbackInfo,
    InputStreamTimestamp, OutputCallbackInfo, OutputStreamTimestamp, PlayStreamError, SampleFormat,
    StreamError, StreamInstant,
};

pub enum Stream {
    Playback(pulseaudio::PlaybackStream),
    Record(pulseaudio::RecordStream),
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        match self {
            Stream::Playback(stream) => {
                block_on(stream.uncork()).map_err(Into::<BackendSpecificError>::into)?;
            }
            Stream::Record(stream) => {
                block_on(stream.uncork()).map_err(Into::<BackendSpecificError>::into)?;
                block_on(stream.started()).map_err(Into::<BackendSpecificError>::into)?;
            }
        };

        Ok(())
    }

    fn pause(&self) -> Result<(), crate::PauseStreamError> {
        let res = match self {
            Stream::Playback(stream) => block_on(stream.cork()),
            Stream::Record(stream) => block_on(stream.cork()),
        };

        res.map_err(Into::<BackendSpecificError>::into)?;
        Ok(())
    }
}

impl Stream {
    pub fn new_playback<D, E>(
        client: pulseaudio::Client,
        params: protocol::PlaybackStreamParams,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<Self, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        // Use a monotonic clock relative to stream creation for StreamInstants.
        let start = std::time::Instant::now();

        let current_latency_micros = Arc::new(AtomicU64::new(0));
        // Microseconds since stream creation at the time of the last latency poll, used
        // to interpolate the latency between polls.
        let last_poll_micros = Arc::new(AtomicU64::new(0));
        let latency_clone = current_latency_micros.clone();
        let poll_clone = last_poll_micros.clone();
        let sample_spec = params.sample_spec;

        let format: SampleFormat = sample_spec
            .format
            .try_into()
            .map_err(|_| BuildStreamError::StreamConfigNotSupported)?;

        // Silence for unsigned formats is the midpoint, not zero. Among
        // PulseAudio's supported formats, only U8 is unsigned and has a
        // single-byte repeatable silence representation (0x80). Multi-byte
        // unsigned formats (U16, U32, ...) are not currently supported.
        let silence_byte = if format == SampleFormat::U8 {
            0x80u8
        } else {
            0u8
        };

        // Wrap the write callback to match the pulseaudio signature.
        let callback = move |buf: &mut [u8]| {
            let elapsed = std::time::Instant::now().saturating_duration_since(start);
            let elapsed_usec = elapsed.as_micros() as u64;

            // Interpolate the latency based on elapsed time since the last
            // poll: as audio plays, the DAC drains the buffer at a constant
            // rate, so the latency decreases linearly between polls.
            let stored_latency = latency_clone.load(atomic::Ordering::Relaxed);
            let poll_usec = poll_clone.load(atomic::Ordering::Relaxed);
            let elapsed_since_poll = elapsed_usec.saturating_sub(poll_usec);
            let latency = stored_latency.saturating_sub(elapsed_since_poll);

            let playback_time = elapsed + time::Duration::from_micros(latency);

            let timestamp = OutputStreamTimestamp {
                callback: StreamInstant {
                    secs: elapsed.as_secs() as i64,
                    nanos: elapsed.subsec_nanos(),
                },
                playback: StreamInstant {
                    secs: playback_time.as_secs() as i64,
                    nanos: playback_time.subsec_nanos(),
                },
            };

            // Preemptively fill the buffer with silence in case the user
            // callback doesn't fill it completely (cpal's API doesn't allow
            // short writes).
            buf.fill(silence_byte);

            let bps = sample_spec.format.bytes_per_sample();
            let n_samples = buf.len() / bps;

            // SAFETY: we calculated the number of samples based on
            // `sample_spec.format`, and `format` is directly derived from (and
            // equivalent to) `sample_spec.format`.
            let mut data = unsafe { Data::from_parts(buf.as_mut_ptr().cast(), n_samples, format) };

            data_callback(&mut data, &OutputCallbackInfo { timestamp });

            // We always consider the full buffer filled, because cpal's
            // user-facing API doesn't allow short writes.
            buf.len()
        };

        let stream = block_on(client.create_playback_stream(params, callback.as_playback_source()))
            .map_err(Into::<BackendSpecificError>::into)?;

        // Share the error callback between the worker and latency threads so
        // both can surface errors to the user.
        let error_callback = Arc::new(Mutex::new(error_callback));

        // Spawn a thread to drive the stream future. It will exit automatically
        // when the stream is stopped by the user.
        let stream_clone = stream.clone();
        let error_callback_clone = error_callback.clone();
        std::thread::spawn(move || {
            if let Err(e) = block_on(stream_clone.play_all()) {
                error_callback_clone.lock().unwrap()(StreamError::from(BackendSpecificError {
                    description: e.to_string(),
                }));
            }
        });

        // Spawn a thread to monitor the stream's latency in a loop. It will
        // exit automatically when the stream ends.
        let stream_clone = stream.clone();
        let latency_clone = current_latency_micros.clone();
        let poll_clone = last_poll_micros.clone();
        std::thread::spawn(move || loop {
            let timing_info = match block_on(stream_clone.timing_info()) {
                Ok(timing_info) => timing_info,
                Err(e) => {
                    error_callback.lock().unwrap()(StreamError::from(BackendSpecificError {
                        description: e.to_string(),
                    }));
                    break;
                }
            };

            let poll_since_epoch = std::time::Instant::now()
                .saturating_duration_since(start)
                .as_micros() as u64;
            poll_clone.store(poll_since_epoch, atomic::Ordering::Relaxed);

            store_latency(
                &latency_clone,
                sample_spec,
                timing_info.sink_usec,
                timing_info.write_offset,
                timing_info.read_offset,
            );

            std::thread::sleep(time::Duration::from_millis(5));
        });

        Ok(Self::Playback(stream))
    }

    pub fn new_record<D, E>(
        client: pulseaudio::Client,
        params: protocol::RecordStreamParams,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Self, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let start = std::time::Instant::now();

        let current_latency_micros = Arc::new(AtomicU64::new(0));
        let latency_clone = current_latency_micros.clone();
        let sample_spec = params.sample_spec;

        let format: SampleFormat = sample_spec
            .format
            .try_into()
            .map_err(|_| BuildStreamError::StreamConfigNotSupported)?;

        let callback = move |buf: &[u8]| {
            let elapsed = std::time::Instant::now().saturating_duration_since(start);
            let latency = latency_clone.load(atomic::Ordering::Relaxed);
            let capture_time = elapsed
                .checked_sub(time::Duration::from_micros(latency))
                .unwrap_or_default();

            let timestamp = InputStreamTimestamp {
                callback: StreamInstant {
                    secs: elapsed.as_secs() as i64,
                    nanos: elapsed.subsec_nanos(),
                },
                capture: StreamInstant {
                    secs: capture_time.as_secs() as i64,
                    nanos: capture_time.subsec_nanos(),
                },
            };

            let bps = sample_spec.format.bytes_per_sample();
            let n_samples = buf.len() / bps;

            // SAFETY: we calculated the number of samples based on
            // `sample_spec.format`, and `format` is directly derived from (and
            // equivalent to) `sample_spec.format`. The pointer is cast from
            // *const to *mut, but cpal's Data type for input streams only
            // exposes shared references (&[T]), so no mutation occurs.
            let data = unsafe { Data::from_parts(buf.as_ptr() as *mut _, n_samples, format) };

            data_callback(&data, &InputCallbackInfo { timestamp });
        };

        let stream = block_on(client.create_record_stream(params, callback))
            .map_err(Into::<BackendSpecificError>::into)?;

        // Spawn a thread to monitor the stream's latency in a loop. It will
        // exit automatically when the stream ends.
        let stream_clone = stream.clone();
        let latency_clone = current_latency_micros.clone();
        std::thread::spawn(move || loop {
            let timing_info = match block_on(stream_clone.timing_info()) {
                Ok(timing_info) => timing_info,
                Err(e) => {
                    error_callback(StreamError::from(BackendSpecificError {
                        description: e.to_string(),
                    }));
                    break;
                }
            };

            store_latency(
                &latency_clone,
                sample_spec,
                timing_info.source_usec,
                timing_info.write_offset,
                timing_info.read_offset,
            );

            std::thread::sleep(time::Duration::from_millis(5));
        });

        Ok(Self::Record(stream))
    }
}

fn store_latency(
    latency_micros: &AtomicU64,
    sample_spec: protocol::SampleSpec,
    device_latency_usec: u64,
    write_offset: i64,
    read_offset: i64,
) {
    let offset = (write_offset - read_offset).max(0) as u64;

    let latency = time::Duration::from_micros(device_latency_usec)
        + sample_spec.bytes_to_duration(offset as usize);

    latency_micros.store(
        latency.as_micros().try_into().unwrap_or(u64::MAX),
        atomic::Ordering::Relaxed,
    );
}
