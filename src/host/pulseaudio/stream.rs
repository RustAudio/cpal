use std::{
    sync::{
        atomic::{self, AtomicBool, AtomicU64},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

use futures::executor::block_on;
use pulseaudio::{protocol, AsPlaybackSource};

use crate::{
    host::{emit_error, ErrorCallbackArc},
    traits::StreamTrait,
    Data, Error, ErrorKind, FrameCount, InputCallbackInfo, InputStreamTimestamp,
    OutputCallbackInfo, OutputStreamTimestamp, SampleFormat, StreamInstant,
};

const LATENCY_MAX_INTERVAL: Duration = Duration::from_millis(100);

// Coordinates the latency polling thread
struct LatencyHandle {
    // Cancellation on drop
    cancel: Arc<AtomicBool>,
    // Event-driven early wakeup from callbacks and play/pause
    update: Arc<(Mutex<bool>, Condvar)>,
}

impl LatencyHandle {
    fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            update: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    // Trigger an early poll
    fn notify(&self) {
        let (lock, cvar) = &*self.update;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cvar.notify_one();
    }

    // Signal cancellation and wake the thread immediately
    fn cancel(&self) {
        self.cancel.store(true, atomic::Ordering::Relaxed);
        self.notify();
    }
}

enum StreamInner {
    Playback(pulseaudio::PlaybackStream, Instant, LatencyHandle),
    Record(pulseaudio::RecordStream, Instant, LatencyHandle),
}

pub struct Stream(StreamInner);

impl Drop for Stream {
    fn drop(&mut self) {
        match &mut self.0 {
            StreamInner::Playback(_, _, handle) | StreamInner::Record(_, _, handle) => {
                handle.cancel()
            }
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        match &self.0 {
            StreamInner::Playback(stream, _, handle) => {
                block_on(stream.uncork()).map_err(Error::from)?;
                handle.notify();
            }
            StreamInner::Record(stream, _, handle) => {
                block_on(stream.uncork()).map_err(Error::from)?;
                block_on(stream.started()).map_err(Error::from)?;
                handle.notify();
            }
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        let res = match &self.0 {
            StreamInner::Playback(stream, _, _) => block_on(stream.cork()),
            StreamInner::Record(stream, _, _) => block_on(stream.cork()),
        };
        res.map_err(Error::from)?;
        match &self.0 {
            StreamInner::Playback(_, _, handle) | StreamInner::Record(_, _, handle) => {
                handle.notify()
            }
        }
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        let start = match &self.0 {
            StreamInner::Playback(_, start, _) | StreamInner::Record(_, start, _) => *start,
        };
        let elapsed = start.elapsed();
        StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos())
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        let (spec, bytes) = match &self.0 {
            StreamInner::Playback(s, _, _) => (
                s.sample_spec(),
                s.buffer_attr().minimum_request_length as usize,
            ),
            StreamInner::Record(s, _, _) => {
                (s.sample_spec(), s.buffer_attr().fragment_size as usize)
            }
        };
        let frame_size = spec.channels as usize * spec.format.bytes_per_sample();
        Ok((bytes / frame_size) as _)
    }
}

impl Stream {
    pub fn new_playback<D, E>(
        client: pulseaudio::Client,
        params: protocol::PlaybackStreamParams,
        mut data_callback: D,
        error_callback: E,
    ) -> Result<Self, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let start = Instant::now();

        let current_latency_micros = Arc::new(AtomicU64::new(0));
        // Microseconds since stream creation at the time of the last latency poll, used
        // to interpolate the latency between polls.
        let last_poll_micros = Arc::new(AtomicU64::new(0));
        let latency_clone = current_latency_micros.clone();
        let poll_clone = last_poll_micros.clone();
        let sample_spec = params.sample_spec;
        let pa_format = sample_spec.format;

        let format: SampleFormat = pa_format.try_into().map_err(|_| {
            Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!("PulseAudio sample format {pa_format:?} is not supported"),
            )
        })?;

        // Silence for unsigned formats is the midpoint, not zero. Among
        // PulseAudio's supported formats, only U8 is unsigned and has a
        // single-byte repeatable silence representation (0x80). Multi-byte
        // unsigned formats (U16, U32, ...) are not currently supported.
        let silence_byte = if format == SampleFormat::U8 {
            0x80u8
        } else {
            0u8
        };

        let handle = LatencyHandle::new();
        let update_callback = handle.update.clone();

        // Wrap the write callback to match the pulseaudio signature.
        let callback = move |buf: &mut [u8]| {
            let elapsed = Instant::now().saturating_duration_since(start);
            let elapsed_usec = elapsed.as_micros() as u64;

            // Interpolate the latency based on elapsed time since the last
            // poll: as audio plays, the DAC drains the buffer at a constant
            // rate, so the latency decreases linearly between polls.
            let stored_latency = latency_clone.load(atomic::Ordering::Relaxed);
            let poll_usec = poll_clone.load(atomic::Ordering::Relaxed);
            // Cap to LATENCY_MAX_INTERVAL: the linear-drain assumption is only valid for that
            // window, and a stale poll_usec (e.g. after cork/uncork where timing_info blocks)
            // would otherwise saturate latency to zero.
            let elapsed_since_poll = elapsed_usec
                .saturating_sub(poll_usec)
                .min(LATENCY_MAX_INTERVAL.as_micros() as u64);
            let latency = stored_latency.saturating_sub(elapsed_since_poll);

            let playback_time = elapsed + Duration::from_micros(latency);

            let timestamp = OutputStreamTimestamp {
                callback: StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos()),
                playback: StreamInstant::new(playback_time.as_secs(), playback_time.subsec_nanos()),
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

            // Notify the latency thread that audio was written, so it updates timing info.
            let (lock, cvar) = &*update_callback;
            *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
            cvar.notify_one();

            // We always consider the full buffer filled, because cpal's
            // user-facing API doesn't allow short writes.
            buf.len()
        };

        let stream = block_on(client.create_playback_stream(params, callback.as_playback_source()))
            .map_err(Error::from)?;

        // Share the error callback between the worker and latency threads so
        // both can surface errors to the user.
        let error_callback: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));

        // Spawn a thread to drive the stream future. It will exit automatically
        // when the stream is stopped by the user.
        let stream_clone = stream.clone();
        let error_callback_clone = error_callback.clone();

        // The barrier prevents the worker and latency threads from firing callbacks before the
        // caller has received the Stream handle.
        let ready = std::sync::Arc::new(std::sync::Barrier::new(3));

        let ready_worker = ready.clone();
        std::thread::spawn(move || {
            ready_worker.wait();
            if let Err(e) = block_on(stream_clone.play_all()) {
                emit_error(&error_callback_clone, Error::from(e));
            }
        });

        let cancel_thread = handle.cancel.clone();
        let update_thread = handle.update.clone();
        let stream_clone = stream.clone();
        let latency_clone = current_latency_micros.clone();
        let poll_clone = last_poll_micros.clone();

        let ready_latency = ready.clone();
        std::thread::spawn(move || {
            ready_latency.wait();
            loop {
                if cancel_thread.load(atomic::Ordering::Relaxed) {
                    break;
                }

                let timing_info = match block_on(stream_clone.timing_info()) {
                    Ok(timing_info) => timing_info,
                    Err(e) => {
                        emit_error(&error_callback, Error::from(e));
                        break;
                    }
                };

                let poll_since_epoch =
                    Instant::now().saturating_duration_since(start).as_micros() as u64;
                poll_clone.store(poll_since_epoch, atomic::Ordering::Relaxed);

                store_latency(
                    &latency_clone,
                    sample_spec,
                    timing_info.sink_usec,
                    timing_info.write_offset,
                    timing_info.read_offset,
                );

                // Wait until woken by a write/play/pause/drop event or until LATENCY_MAX_INTERVAL.
                let (lock, cvar) = &*update_thread;
                let Ok(guard) = lock.lock() else { break };
                let (mut guard, _) = cvar
                    .wait_timeout_while(guard, LATENCY_MAX_INTERVAL, |notified| !*notified)
                    .unwrap_or_else(|e| e.into_inner());
                *guard = false;
            }
        });

        ready.wait();
        Ok(Self(StreamInner::Playback(stream, start, handle)))
    }

    pub fn new_record<D, E>(
        client: pulseaudio::Client,
        params: protocol::RecordStreamParams,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Self, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let start = Instant::now();

        let current_latency_micros = Arc::new(AtomicU64::new(0));
        // Microseconds since stream creation at the time of the last latency poll, used
        // to interpolate the latency between polls.
        let last_poll_micros = Arc::new(AtomicU64::new(0));
        let latency_clone = current_latency_micros.clone();
        let poll_clone = last_poll_micros.clone();
        let sample_spec = params.sample_spec;
        let pa_format = sample_spec.format;

        let format: SampleFormat = pa_format.try_into().map_err(|_| {
            Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!("PulseAudio sample format {pa_format:?} is not supported"),
            )
        })?;

        let handle = LatencyHandle::new();
        let update_callback = handle.update.clone();

        let callback = move |buf: &[u8]| {
            let elapsed = Instant::now().saturating_duration_since(start);
            let elapsed_usec = elapsed.as_micros() as u64;

            // Interpolate the latency based on elapsed time since the last poll: as audio records,
            // the ADC fills the buffer at a constant rate, so the latency increases linearly
            // between polls.
            let stored_latency = latency_clone.load(atomic::Ordering::Relaxed);
            let poll_usec = poll_clone.load(atomic::Ordering::Relaxed);
            // Cap to LATENCY_MAX_INTERVAL: the linear-fill assumption is only valid for that
            // window, and a stale poll_usec (e.g. after cork/uncork where timing_info blocks)
            // would otherwise keep inflating the interpolated latency up to the cap.
            let elapsed_since_poll = elapsed_usec
                .saturating_sub(poll_usec)
                .min(LATENCY_MAX_INTERVAL.as_micros() as u64);
            let latency = stored_latency.saturating_add(elapsed_since_poll);

            let capture_time = elapsed
                .checked_sub(Duration::from_micros(latency))
                .unwrap_or_default();

            let timestamp = InputStreamTimestamp {
                callback: StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos()),
                capture: StreamInstant::new(capture_time.as_secs(), capture_time.subsec_nanos()),
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

            // Notify the latency thread that audio was read, so it updates timing info.
            let (lock, cvar) = &*update_callback;
            *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
            cvar.notify_one();
        };

        let stream =
            block_on(client.create_record_stream(params, callback)).map_err(Error::from)?;

        // Spawn a thread to monitor the stream's latency in a loop.
        let cancel_thread = handle.cancel.clone();
        let update_thread = handle.update.clone();
        let stream_clone = stream.clone();
        let latency_clone = current_latency_micros.clone();
        let poll_clone = last_poll_micros.clone();
        std::thread::spawn(move || loop {
            if cancel_thread.load(atomic::Ordering::Relaxed) {
                break;
            }

            let timing_info = match block_on(stream_clone.timing_info()) {
                Ok(timing_info) => timing_info,
                Err(e) => {
                    error_callback(Error::from(e));
                    break;
                }
            };

            let poll_since_epoch =
                Instant::now().saturating_duration_since(start).as_micros() as u64;
            poll_clone.store(poll_since_epoch, atomic::Ordering::Relaxed);

            store_latency(
                &latency_clone,
                sample_spec,
                timing_info.source_usec,
                timing_info.write_offset,
                timing_info.read_offset,
            );

            // Wait until woken by a read/play/pause/drop event or until LATENCY_MAX_INTERVAL.
            let (lock, cvar) = &*update_thread;
            let Ok(guard) = lock.lock() else { break };
            let (mut guard, _) = cvar
                .wait_timeout_while(guard, LATENCY_MAX_INTERVAL, |notified| !*notified)
                .unwrap_or_else(|e| e.into_inner());
            *guard = false;
        });

        Ok(Self(StreamInner::Record(stream, start, handle)))
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

    let latency =
        Duration::from_micros(device_latency_usec) + sample_spec.bytes_to_duration(offset as usize);

    latency_micros.store(
        latency.as_micros().try_into().unwrap_or(u64::MAX),
        atomic::Ordering::Relaxed,
    );
}
