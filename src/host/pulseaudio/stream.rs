use std::{
    sync::{
        atomic::{self, AtomicU64},
        Arc,
    },
    time::{self, SystemTime},
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
        mut error_callback: E,
    ) -> Result<Self, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let epoch = std::time::SystemTime::now();

        let current_latency_micros = Arc::new(AtomicU64::new(0));
        let latency_clone = current_latency_micros.clone();
        let sample_spec = params.sample_spec;

        let format: SampleFormat = sample_spec
            .format
            .try_into()
            .map_err(|_| BuildStreamError::StreamConfigNotSupported)?;

        // Wrap the write callback to match the pulseaudio signature.
        let callback = move |buf: &mut [u8]| {
            let now = SystemTime::now().duration_since(epoch).unwrap_or_default();
            let latency = latency_clone.load(atomic::Ordering::Relaxed);
            let playback_time = now + time::Duration::from_micros(latency);

            let timestamp = OutputStreamTimestamp {
                callback: StreamInstant {
                    secs: now.as_secs() as i64,
                    nanos: now.subsec_nanos(),
                },
                playback: StreamInstant {
                    secs: playback_time.as_secs() as i64,
                    nanos: playback_time.subsec_nanos(),
                },
            };

            // Preemptively zero the buffer.
            for b in buf.iter_mut() {
                *b = 0;
            }

            let bps = sample_spec.format.bytes_per_sample();
            let n_samples = buf.len() / bps;

            // SAFETY: we calculated the number of samples based on
            // `sample_spec.format`, and `format` is directly derived from (and
            // equivalent to) `sample_spec.format`.
            let mut data = unsafe { Data::from_parts(buf.as_mut_ptr().cast(), n_samples, format) };

            data_callback(&mut data, &OutputCallbackInfo { timestamp });

            // We always consider the full buffer filled, because cpal's
            // user-facing api doesn't allow for short writes.
            n_samples * bps
        };

        let stream = block_on(client.create_playback_stream(params, callback.as_playback_source()))
            .map_err(Into::<BackendSpecificError>::into)?;

        // Spawn a thread to drive the stream future. It will exit automatically
        // when the stream is stopped by the user.
        let stream_clone = stream.clone();
        let _worker_thread = std::thread::spawn(move || block_on(stream_clone.play_all()));

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
                timing_info.sink_usec,
                timing_info.write_offset,
                timing_info.read_offset,
            );

            std::thread::sleep(time::Duration::from_millis(100));
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
        let epoch = std::time::SystemTime::now();

        let current_latency_micros = Arc::new(AtomicU64::new(0));
        let latency_clone = current_latency_micros.clone();
        let sample_spec = params.sample_spec;

        let format: SampleFormat = sample_spec
            .format
            .try_into()
            .map_err(|_| BuildStreamError::StreamConfigNotSupported)?;

        let callback = move |buf: &[u8]| {
            let now = SystemTime::now().duration_since(epoch).unwrap_or_default();
            let latency = latency_clone.load(atomic::Ordering::Relaxed);
            let capture_time = now
                .checked_sub(time::Duration::from_micros(latency))
                .unwrap_or_default();

            let timestamp = InputStreamTimestamp {
                callback: StreamInstant {
                    secs: now.as_secs() as i64,
                    nanos: now.subsec_nanos(),
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
            // equivalent to) `sample_spec.format`.
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
                timing_info.sink_usec,
                timing_info.write_offset,
                timing_info.read_offset,
            );

            std::thread::sleep(time::Duration::from_millis(100));
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
) -> time::Duration {
    let offset = (write_offset as u64).saturating_sub(read_offset as u64);

    let latency = time::Duration::from_micros(device_latency_usec)
        + sample_spec.bytes_to_duration(offset as usize);

    latency_micros.store(
        latency.as_micros().try_into().unwrap_or(u64::MAX),
        atomic::Ordering::Relaxed,
    );

    latency
}
