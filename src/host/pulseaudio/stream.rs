use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use futures_executor::block_on;
use futures_util::FutureExt as _;
use pulseaudio::{AsPlaybackSource, protocol};

use crate::{
    CallbackInfo, Data, Error, ErrorKind, FrameCount, SampleFormat, StreamInstant, StreamTimestamp,
    host::{ErrorCallbackArc, emit_error, latch::Latch},
    traits::StreamTrait,
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
        self.cancel.store(true, Ordering::Relaxed);
        self.notify();
    }
}

enum StreamInner {
    Playback {
        stream: pulseaudio::PlaybackStream,
        start: Instant,
        handle: LatencyHandle,
        draining: Arc<AtomicBool>,
        fill_usec: Arc<AtomicU64>,
    },
    Record {
        stream: pulseaudio::RecordStream,
        start: Instant,
        handle: LatencyHandle,
    },
}

pub struct Stream {
    inner: StreamInner,
    workers: Vec<std::thread::JoinHandle<()>>,
    latch: Latch,
}

impl Drop for Stream {
    fn drop(&mut self) {
        match &mut self.inner {
            StreamInner::Playback { stream, handle, .. } => {
                handle.cancel();
                // Help the play_all driver thread terminate by
                // queueing a delete, which causes the reactor to drop
                // the source's eof_tx. We need to do this because
                // poll_read always reports a non-empty buffer.
                let _ = stream.clone().delete().now_or_never();
            }
            StreamInner::Record { handle, .. } => {
                handle.cancel();
            }
        }

        // Unpark the threads in case they're sleeping.
        self.signal_ready();

        for handle in self.workers.drain(..) {
            // Prevent self-join: a worker thread may surface an error
            // through the user's error_callback, and that callback may
            // drop the Stream — in which case we'd be joining ourselves.
            if handle.thread().id() != std::thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        match &self.inner {
            StreamInner::Playback {
                stream,
                handle,
                draining,
                ..
            } => {
                // Clear any pending drain so the write callback resumes pulling real audio.
                draining.store(false, Ordering::Relaxed);
                block_on(stream.uncork()).map_err(Error::from)?;
                handle.notify();
            }
            StreamInner::Record { stream, handle, .. } => {
                block_on(stream.uncork()).map_err(Error::from)?;
                block_on(stream.started()).map_err(Error::from)?;
                handle.notify();
            }
        }
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        let res = match &self.inner {
            StreamInner::Playback { stream, .. } => block_on(stream.cork()),
            StreamInner::Record { stream, .. } => block_on(stream.cork()),
        };
        res.map_err(Error::from)?;
        match &self.inner {
            StreamInner::Playback { handle, .. } | StreamInner::Record { handle, .. } => {
                handle.notify();
            }
        }
        Ok(())
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        match &self.inner {
            StreamInner::Playback {
                stream,
                handle,
                draining,
                fill_usec,
                ..
            } => {
                // TODO: use PulseAudio's drain() when https://github.com/colinmarc/pulseaudio-rs/pull/9 is merged.
                draining.store(true, Ordering::Relaxed);
                if timeout != Some(Duration::ZERO) {
                    let buffered = Duration::from_micros(fill_usec.load(Ordering::Relaxed));
                    let wait = timeout.map_or(buffered, |t| buffered.min(t));
                    if !wait.is_zero() {
                        std::thread::sleep(wait);
                    }
                }
                block_on(stream.cork()).map_err(Error::from)?;
                block_on(stream.flush()).map_err(Error::from)?;
                handle.notify();
            }
            StreamInner::Record { stream, handle, .. } => {
                block_on(stream.cork()).map_err(Error::from)?;
                block_on(stream.flush()).map_err(Error::from)?;
                handle.notify();
            }
        }
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        let start = match &self.inner {
            StreamInner::Playback { start, .. } | StreamInner::Record { start, .. } => *start,
        };
        let elapsed = start.elapsed();
        StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos())
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        let (spec, bytes) = match &self.inner {
            StreamInner::Playback { stream, .. } => (
                stream.sample_spec(),
                stream.buffer_attr().minimum_request_length as usize,
            ),
            StreamInner::Record { stream, .. } => (
                stream.sample_spec(),
                stream.buffer_attr().fragment_size as usize,
            ),
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
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
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
                "Sample format is not supported",
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

        let draining = Arc::new(AtomicBool::new(false));
        let draining_callback = draining.clone();

        let fill_usec = Arc::new(AtomicU64::new(0));
        let fill_usec_clone = fill_usec.clone();

        // Wrap the write callback to match the pulseaudio signature.
        let callback = move |buf: &mut [u8]| {
            // Preemptively fill the buffer with silence in case the user
            // callback doesn't fill it completely (cpal's API doesn't allow
            // short writes).
            buf.fill(silence_byte);

            // While draining (set by stop()), leave the buffer at the silence fill above and skip
            // the user callback, so previously queued audio plays out and new output is silence
            // until start() clears the flag. Still report the buffer as fully written (never 0,
            // which the reactor treats as source EOF) so the stream keeps being serviced normally.
            if !draining_callback.load(Ordering::Relaxed) {
                let elapsed = Instant::now().saturating_duration_since(start);
                let elapsed_usec = elapsed.as_micros() as u64;

                // Interpolate the latency based on elapsed time since the last
                // poll: as audio plays, the DAC drains the buffer at a constant
                // rate, so the latency decreases linearly between polls.
                let stored_latency = latency_clone.load(Ordering::Relaxed);
                let poll_usec = poll_clone.load(Ordering::Relaxed);
                // Cap to LATENCY_MAX_INTERVAL: the linear-drain assumption is only valid for that
                // window, and a stale poll_usec (e.g. after cork/uncork where timing_info blocks)
                // would otherwise saturate latency to zero.
                let elapsed_since_poll = elapsed_usec
                    .saturating_sub(poll_usec)
                    .min(LATENCY_MAX_INTERVAL.as_micros() as u64);
                let latency = stored_latency.saturating_sub(elapsed_since_poll);

                let playback_time = elapsed + Duration::from_micros(latency);
                let timestamp = StreamTimestamp {
                    callback: StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos()),
                    device: StreamInstant::new(
                        playback_time.as_secs(),
                        playback_time.subsec_nanos(),
                    ),
                };

                let bps = sample_spec.format.bytes_per_sample();
                let n_samples = buf.len() / bps;

                // SAFETY: we calculated the number of samples based on
                // `sample_spec.format`, and `format` is directly derived from (and
                // equivalent to) `sample_spec.format`.
                let mut data =
                    unsafe { Data::from_parts(buf.as_mut_ptr().cast(), n_samples, format) };

                // TODO: real underrun status when https://github.com/colinmarc/pulseaudio-rs/pull/10 is merged.
                data_callback(
                    &mut data,
                    &CallbackInfo {
                        timestamp,
                        xrun: false,
                    },
                );

                // Notify the latency thread that audio was written, so it updates timing info.
                let (lock, cvar) = &*update_callback;
                *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
                cvar.notify_one();
            }

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
        let cancel_driver = handle.cancel.clone();

        // The latch is released just before the `Stream` is returned so the driver and latency
        // threads cannot fire any callbacks before the caller has the handle.
        let mut latch = Latch::new();
        let waiter_driver = latch.waiter();

        let driver_handle = std::thread::spawn(move || {
            waiter_driver.wait();
            if let Err(e) = block_on(stream_clone.play_all()) {
                // A server playback error is expected when the client
                // closes their stream. No need to report it back to
                // the client.
                if !cancel_driver.load(Ordering::Relaxed) {
                    emit_error(&error_callback_clone, Error::from(e));
                }
            }
        });

        let cancel_thread = handle.cancel.clone();
        let update_thread = handle.update.clone();
        let stream_clone = stream.clone();
        let latency_clone = current_latency_micros.clone();
        let poll_clone = last_poll_micros.clone();

        let waiter_latency = latch.waiter();
        let latency_handle = std::thread::spawn(move || {
            waiter_latency.wait();
            loop {
                if cancel_thread.load(Ordering::Relaxed) {
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
                poll_clone.store(poll_since_epoch, Ordering::Relaxed);

                store_latency(
                    &latency_clone,
                    Some(&fill_usec_clone),
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

        latch.add_thread(driver_handle.thread().clone());
        latch.add_thread(latency_handle.thread().clone());
        Ok(Self {
            inner: StreamInner::Playback {
                stream,
                start,
                handle,
                draining,
                fill_usec,
            },
            workers: vec![driver_handle, latency_handle],
            latch,
        })
    }

    pub fn new_record<D, E>(
        client: pulseaudio::Client,
        params: protocol::RecordStreamParams,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Result<Self, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
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
                "Sample format is not supported",
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
            let stored_latency = latency_clone.load(Ordering::Relaxed);
            let poll_usec = poll_clone.load(Ordering::Relaxed);
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

            let timestamp = StreamTimestamp {
                callback: StreamInstant::new(elapsed.as_secs(), elapsed.subsec_nanos()),
                device: StreamInstant::new(capture_time.as_secs(), capture_time.subsec_nanos()),
            };

            let bps = sample_spec.format.bytes_per_sample();
            let n_samples = buf.len() / bps;

            // SAFETY: we calculated the number of samples based on
            // `sample_spec.format`, and `format` is directly derived from (and
            // equivalent to) `sample_spec.format`. The pointer is cast from
            // *const to *mut, but cpal's Data type for input streams only
            // exposes shared references (&[T]), so no mutation occurs.
            let data = unsafe { Data::from_parts(buf.as_ptr() as *mut _, n_samples, format) };

            // TODO: real overrun status when https://github.com/colinmarc/pulseaudio-rs/pull/10 is merged.
            data_callback(
                &data,
                &CallbackInfo {
                    timestamp,
                    xrun: false,
                },
            );

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

        // The latch is released just before the `Stream` is returned so the latency thread cannot
        // fire any callbacks before the caller has the handle.
        let mut latch = Latch::new();
        let waiter_latency = latch.waiter();

        let latency_handle = std::thread::spawn(move || {
            waiter_latency.wait();
            loop {
                if cancel_thread.load(Ordering::Relaxed) {
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
                poll_clone.store(poll_since_epoch, Ordering::Relaxed);

                store_latency(
                    &latency_clone,
                    None,
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
            }
        });

        latch.add_thread(latency_handle.thread().clone());
        Ok(Self {
            inner: StreamInner::Record {
                stream,
                start,
                handle,
            },
            workers: vec![latency_handle],
            latch,
        })
    }

    /// Releases the latch so the worker thread can begin processing audio callbacks.
    pub(crate) fn signal_ready(&self) {
        self.latch.release();
    }
}

fn store_latency(
    latency_micros: &AtomicU64,
    fill_usec: Option<&AtomicU64>,
    sample_spec: protocol::SampleSpec,
    device_latency_usec: u64,
    write_offset: i64,
    read_offset: i64,
) {
    let offset = (write_offset - read_offset).max(0) as u64;
    let buffer_usec: u64 = sample_spec
        .bytes_to_duration(offset as usize)
        .as_micros()
        .try_into()
        .unwrap_or(u64::MAX);

    if let Some(fill) = fill_usec {
        fill.store(
            device_latency_usec.saturating_add(buffer_usec),
            Ordering::Relaxed,
        );
    }
    latency_micros.store(
        device_latency_usec.saturating_add(buffer_usec),
        Ordering::Relaxed,
    );
}
