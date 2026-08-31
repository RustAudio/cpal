use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use super::{
    alsa,
    alsa::poll::Descriptors,
    poll_timeout_millis,
    stream::{ErrorCallback, InputDataCallback, OutputDataCallback, StreamInner},
    timestamp::status_with_timestamp,
    trigger::TriggerReceiver,
};
use crate::{
    CallbackInfo, Data, Error, ErrorKind, FrameCount, StreamInstant, StreamTimestamp,
    host::frames_to_duration,
};

pub(super) fn input_stream_worker(
    rx: Arc<TriggerReceiver>,
    stream: &StreamInner,
    data_callback: &mut InputDataCallback,
    error_callback: &mut ErrorCallback,
    timeout: Option<Duration>,
) {
    #[cfg(feature = "realtime")]
    if stream.is_rt_eligible() {
        let period_frames = u32::try_from(stream.period_size).unwrap_or(0);
        if let Err(err) = audio_thread_priority::promote_current_thread_to_real_time(
            period_frames,
            stream.sample_rate,
        ) {
            error_callback(err.into());
        }
    }

    let mut ctxt = StreamWorkerContext::new(&timeout, stream, &rx);
    loop {
        if stream.control.dropping.load(Ordering::Relaxed) {
            return;
        }
        if stream.control.parked.load(Ordering::Relaxed) {
            stream.control.acknowledge_park();
        }
        let result = match poll_for_period(&rx, stream, &mut ctxt) {
            Ok(Poll::Pending) => continue,
            Ok(Poll::Recover) => recover_input(stream),
            Ok(Poll::Ready {
                status,
                delay_frames,
            }) => process_input(
                stream,
                &mut ctxt.transfer_buffer,
                status,
                delay_frames,
                data_callback,
            ),
            Err(err) => Err(err),
        };
        if let Err(err) = result {
            match err.kind() {
                ErrorKind::DeviceNotAvailable => {
                    error_callback(err);
                    stream.control.signal_worker_exit();
                    return;
                }
                _ => error_callback(err),
            }
        }
    }
}

pub(super) fn output_stream_worker(
    rx: Arc<TriggerReceiver>,
    stream: &StreamInner,
    data_callback: &mut OutputDataCallback,
    error_callback: &mut ErrorCallback,
    timeout: Option<Duration>,
) {
    #[cfg(feature = "realtime")]
    if stream.is_rt_eligible() {
        let period_frames = u32::try_from(stream.period_size).unwrap_or(0);
        if let Err(err) = audio_thread_priority::promote_current_thread_to_real_time(
            period_frames,
            stream.sample_rate,
        ) {
            error_callback(err.into());
        }
    }

    let mut ctxt = StreamWorkerContext::new(&timeout, stream, &rx);

    loop {
        if stream.control.dropping.load(Ordering::Relaxed) {
            return;
        }
        if stream.control.parked.load(Ordering::Relaxed) {
            stream.control.acknowledge_park();
        }
        let result = match poll_for_period(&rx, stream, &mut ctxt) {
            Ok(Poll::Pending) => continue,
            Ok(Poll::Recover) => recover_output(stream),
            Ok(Poll::Ready {
                status,
                delay_frames,
            }) => process_output(
                stream,
                &mut ctxt.transfer_buffer,
                status,
                delay_frames,
                data_callback,
            ),
            Err(err) => Err(err),
        };
        if let Err(err) = result {
            match err.kind() {
                ErrorKind::DeviceNotAvailable => {
                    error_callback(err);
                    stream.control.signal_worker_exit();
                    return;
                }
                _ => error_callback(err),
            }
        }
    }
}

struct StreamWorkerContext {
    descriptors: Box<[libc::pollfd]>,
    transfer_buffer: Box<[u8]>,
    poll_timeout: i32,
}

impl StreamWorkerContext {
    fn new(poll_timeout: &Option<Duration>, stream: &StreamInner, rx: &TriggerReceiver) -> Self {
        let poll_timeout = poll_timeout_millis(*poll_timeout);

        // Pre-allocate a period-sized working buffer. Contents are overwritten each callback.
        let transfer_buffer = vec![0u8; stream.period_size * stream.frame_size].into_boxed_slice();

        // Pre-allocate and initialize descriptors vector: 1 for self-pipe + ALSA descriptors.
        // The descriptor count is constant for the lifetime of stream parameters, and
        // poll() overwrites revents on each call, so we only need to set up fd and events once.
        let num_descriptors = stream.handle.count();
        let total_descriptors = 1 + num_descriptors;
        let mut descriptors = vec![
            libc::pollfd {
                fd: 0,
                events: 0,
                revents: 0
            };
            total_descriptors
        ]
        .into_boxed_slice();

        // Set up self-pipe descriptor at index 0
        descriptors[0] = libc::pollfd {
            fd: rx.0,
            events: libc::POLLIN,
            revents: 0,
        };

        // Set up ALSA descriptors starting at index 1
        let filled = stream
            .handle
            .fill(&mut descriptors[1..])
            .expect("Failed to fill ALSA descriptors");
        debug_assert_eq!(filled, num_descriptors);

        Self {
            descriptors,
            transfer_buffer,
            poll_timeout,
        }
    }
}

/// Attempt hardware resume from a suspend event (`ESTRPIPE`).
fn try_resume(stream: &StreamInner) -> Result<Poll, Error> {
    let handle = &stream.handle;

    let hw_params = handle.hw_params_current()?;
    if !hw_params.can_resume() {
        // Hardware doesn't support suspend/resume: fall back to full recovery.
        stream.pending_xrun.store(true, Ordering::Relaxed);
        return Ok(Poll::Recover);
    }

    match handle.resume() {
        Ok(()) => {
            if handle
                .info()
                .map(|i| i.get_stream() == alsa::Direction::Capture)
                .unwrap_or(false)
            {
                // A successful `resume()` may leave the device `PREPARED` rather than `RUNNING`.
                // `start()` to ensure the capture actually resumes.
                if let Err(e) = handle.start() {
                    // `EBUSY` is ignored because it means the device is already running.
                    if e.errno() != libc::EBUSY {
                        return Err(e.into());
                    }
                }
            }
            Ok(Poll::Pending)
        }
        // device is still resuming; poll again until it is ready.
        Err(e) if e.errno() == libc::EAGAIN => Ok(Poll::Pending),
        // hardware does not support soft resume: fall back to full recovery.
        Err(e) if e.errno() == libc::ENOSYS => {
            stream.pending_xrun.store(true, Ordering::Relaxed);
            Ok(Poll::Recover)
        }
        Err(e) => Err(e.into()),
    }
}

enum Poll {
    Pending,
    Ready {
        status: alsa::pcm::Status,
        delay_frames: usize,
    },
    // An xrun was detected; the worker should call prepare() (+ start() for input) and loop.
    Recover,
}

fn poll_for_period(
    rx: &TriggerReceiver,
    stream: &StreamInner,
    ctxt: &mut StreamWorkerContext,
) -> Result<Poll, Error> {
    let StreamWorkerContext {
        ref mut descriptors,
        ref poll_timeout,
        ..
    } = *ctxt;

    let res = alsa::poll::poll(descriptors, *poll_timeout)?;
    if res == 0 {
        // Timeout expired with no events. Query PCM state to handle cases where
        // POLLERR/POLLHUP was not delivered before the timeout fired (e.g. some
        // power-management suspend paths or VM/container ALSA shims).
        match stream.handle.state() {
            alsa::pcm::State::Disconnected => {
                return Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    "Device disconnected",
                ));
            }
            // Xrun with POLLERR missed: recover the same way the POLLERR path does.
            alsa::pcm::State::XRun => {
                stream.pending_xrun.store(true, Ordering::Relaxed);
                return Ok(Poll::Recover);
            }
            // Suspend with POLLHUP/POLLERR missed: attempt hardware resume.
            alsa::pcm::State::Suspended => return try_resume(stream),
            // No events and no error state: spurious wakeup, poll again.
            _ => {}
        }
        return Ok(Poll::Pending);
    }

    if descriptors[0].revents != 0 {
        // Self-pipe fired: the stream is being dropped. Clear the pipe and let the
        // worker loop detect the dropping flag on the next iteration.
        rx.clear_pipe();
        return Ok(Poll::Pending);
    }

    let revents = stream.handle.revents(&descriptors[1..])?;
    // No events: spurious wakeup, poll again.
    if revents.is_empty() {
        return Ok(Poll::Pending);
    }
    // POLLHUP/POLLNVAL: the device has been disconnected.
    if revents.intersects(alsa::poll::Flags::HUP | alsa::poll::Flags::NVAL) {
        return Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "Device disconnected",
        ));
    }
    // POLLERR signals an xrun or suspend; avail_delay() below returns an error accordingly.
    // POLLIN/POLLOUT: data is ready, fall through to process it.
    let (avail_frames, delay_frames) = match stream.handle.avail_delay() {
        // Suspend: try hardware resume first; fall back to prepare() if unsupported.
        // BSD compat: check via PCM state rather than the Linux-specific ESTRPIPE errno.
        Err(_) if matches!(stream.handle.state(), alsa::pcm::State::Suspended) => {
            return try_resume(stream);
        }
        // Xrun: recover via prepare() (+ start() for capture, handled by the worker).
        Err(err) if err.errno() == libc::EPIPE => {
            stream.pending_xrun.store(true, Ordering::Relaxed);
            return Ok(Poll::Recover);
        }
        res => res,
    }?;
    // ALSA can have spurious wakeups where poll returns but avail < avail_min.
    // This is documented to occur with dmix (timer-driven) and other plugins.
    // Verify we have room for at least one full period before processing.
    // See: https://bugzilla.kernel.org/show_bug.cgi?id=202499
    //
    // Compare in Frames (i64) so that a negative avail_frames from a buggy driver
    // naturally fails the guard rather than wrapping to a huge usize that passes it.
    if avail_frames < stream.period_size as alsa::pcm::Frames {
        return Ok(Poll::Pending);
    }

    // From the guard above we know that this poll is not a spurious wakeup,
    // so we also know we can query the device in a stable state.
    let status = status_with_timestamp(&stream.handle, stream.timestamp_mode)?;

    Ok(Poll::Ready {
        status,
        delay_frames: delay_frames.max(0) as usize,
    })
}

// Full input underrun recovery: mark the xrun, then prepare + start the stream.
fn recover_input(stream: &StreamInner) -> Result<(), Error> {
    stream.pending_xrun.store(true, Ordering::Relaxed);
    stream.handle.prepare()?;
    stream.handle.start()?;
    Ok(())
}

// Read input data from ALSA and deliver it to the user.
fn process_input(
    stream: &StreamInner,
    buffer: &mut [u8],
    status: alsa::pcm::Status,
    delay_frames: usize,
    data_callback: &mut InputDataCallback,
) -> Result<(), Error> {
    let mut frames_read = 0;
    while frames_read < stream.period_size {
        match stream
            .handle
            .io_bytes()
            .readi(&mut buffer[frames_read * stream.frame_size..])
        {
            Ok(n) => frames_read += n,
            // EAGAIN = no frames available: skip this cycle if no progress was made,
            // otherwise treat as an underrun (partial period cannot be delivered safely).
            Err(err) if err.errno() == libc::EAGAIN && frames_read == 0 => return Ok(()),
            // Suspend: try soft resume first, falling back to underrun recovery if the
            // hardware doesn't support it. BSD compat: check via PCM state rather than the
            // Linux-specific ESTRPIPE errno.
            Err(_) if matches!(stream.handle.state(), alsa::pcm::State::Suspended) => {
                return match try_resume(stream)? {
                    Poll::Recover => recover_input(stream),
                    _ => Ok(()),
                };
            }
            // EAGAIN with partial progress, or EPIPE: full underrun recovery required.
            Err(err) if err.errno() == libc::EAGAIN || err.errno() == libc::EPIPE => {
                return recover_input(stream);
            }
            Err(err) => return Err(err.into()),
        }
    }
    if !stream.control.draining.load(Ordering::Relaxed) {
        let data = buffer.as_mut_ptr() as *mut ();
        let data = unsafe { Data::from_parts(data, stream.period_samples, stream.sample_format) };
        let callback_instant = stream.callback_instant(&status);
        let delay_duration = frames_to_duration(delay_frames as FrameCount, stream.sample_rate);
        let capture = callback_instant
            .checked_sub(delay_duration)
            .unwrap_or(StreamInstant::ZERO);
        let timestamp = StreamTimestamp {
            callback: callback_instant,
            device: capture,
        };
        let xrun = stream.pending_xrun.swap(false, Ordering::Relaxed);
        data_callback(&data, &CallbackInfo { timestamp, xrun });
    }

    Ok(())
}

// Request data from the user's function and write it via ALSA.
// Full output underrun recovery: mark the xrun, then prepare the stream. No need to call
// start(): ALSA automatically restarts output streams once the buffer is refilled and
// triggered again.
fn recover_output(stream: &StreamInner) -> Result<(), Error> {
    stream.pending_xrun.store(true, Ordering::Relaxed);
    stream.handle.prepare()?;
    Ok(())
}

fn process_output(
    stream: &StreamInner,
    buffer: &mut [u8],
    status: alsa::pcm::Status,
    delay_frames: usize,
    data_callback: &mut OutputDataCallback,
) -> Result<(), Error> {
    // Pre-fill buffer with equilibrium; user callback overwrites what it wants.
    stream
        .equilibrium
        .as_ref()
        .expect("process_output only runs for Output-direction streams")
        .fill(buffer);

    if !stream.control.draining.load(Ordering::Relaxed) {
        let data = buffer.as_mut_ptr() as *mut ();
        let mut data =
            unsafe { Data::from_parts(data, stream.period_samples, stream.sample_format) };
        let callback_instant = stream.callback_instant(&status);
        let delay_duration = frames_to_duration(delay_frames as FrameCount, stream.sample_rate);
        let playback = callback_instant + delay_duration;
        let timestamp = StreamTimestamp {
            callback: callback_instant,
            device: playback,
        };
        let xrun = stream.pending_xrun.swap(false, Ordering::Relaxed);
        data_callback(&mut data, &CallbackInfo { timestamp, xrun });
    }

    let mut frames_written = 0;
    while frames_written < stream.period_size {
        match stream
            .handle
            .io_bytes()
            .writei(&buffer[frames_written * stream.frame_size..])
        {
            Ok(n) => frames_written += n,
            // EAGAIN = device cannot currently accept more frames: skip this cycle if no
            // progress was made, otherwise treat as an underrun (partial period cannot be
            // completed safely).
            Err(err) if err.errno() == libc::EAGAIN && frames_written == 0 => return Ok(()),
            // Suspend: try soft resume first, falling back to underrun recovery if the
            // hardware doesn't support it. BSD compat: check via PCM state rather than the Linux-specific ESTRPIPE errno.
            Err(_) if matches!(stream.handle.state(), alsa::pcm::State::Suspended) => {
                return match try_resume(stream)? {
                    Poll::Recover => recover_output(stream),
                    _ => Ok(()),
                };
            }
            // EAGAIN with partial progress, or EPIPE: full underrun recovery required.
            Err(err) if err.errno() == libc::EAGAIN || err.errno() == libc::EPIPE => {
                return recover_output(stream);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}
