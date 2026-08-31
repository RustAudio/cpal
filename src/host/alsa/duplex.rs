use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use super::{
    DEFAULT_PERIODS, POLL_INFINITE, alsa,
    alsa::poll::Descriptors,
    stream::DuplexStreamInner,
    timestamp::{callback_instant_for, status_with_timestamp},
    trigger::TriggerReceiver,
};
use crate::{
    CallbackInfo, Data, DuplexCallbackInfo, Error, ErrorKind, FrameCount, StreamInstant,
    StreamTimestamp, host::frames_to_duration,
};

pub(super) fn start_duplex(stream: &DuplexStreamInner) -> Result<(), Error> {
    match stream.capture.handle.state() {
        alsa::pcm::State::Paused => {
            let resumed = stream
                .capture
                .handle
                .pause(false)
                .and_then(|_| stream.playback.handle.pause(false));
            // Mirrors pause_duplex's fallback: resuming a linked pair via PAUSE_RELEASE can be
            // as unreliable as pausing it was, on the same drivers.
            if resumed.is_err() {
                stream.capture.handle.drop().ok();
                stream.playback.handle.drop().ok();
                stream.capture.handle.prepare()?;
                stream.playback.handle.prepare()?;
                begin_duplex_playback(stream)?;
            }
        }
        // Guard against Setup in case prepare() in stop() failed silently.
        alsa::pcm::State::Prepared | alsa::pcm::State::Setup => {
            if stream.capture.handle.state() == alsa::pcm::State::Setup {
                stream.capture.handle.prepare()?;
            }
            if stream.playback.handle.state() == alsa::pcm::State::Setup {
                stream.playback.handle.prepare()?;
            }
            begin_duplex_playback(stream)?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn duplex_stream_worker(
    rx: Arc<TriggerReceiver>,
    stream: &DuplexStreamInner,
    data_callback: &mut (dyn FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static),
    error_callback: &mut (dyn FnMut(Error) + Send + 'static),
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

    let mut ctxt = DuplexStreamWorkerContext::new(&timeout, stream, &rx);
    loop {
        if stream.control.dropping.load(Ordering::Relaxed) {
            return;
        }
        if stream.control.parked.load(Ordering::Relaxed) {
            stream.control.acknowledge_park();
        }
        let result = match poll_for_duplex_period(&rx, stream, &mut ctxt) {
            Ok(DuplexPoll::Pending) => continue,
            Ok(DuplexPoll::Recover) => recover_duplex(stream),
            Ok(DuplexPoll::Ready {
                capture_status,
                playback_status,
                capture_delay,
                playback_delay,
            }) => process_duplex(
                stream,
                &mut ctxt.capture_buffer,
                &mut ctxt.playback_buffer,
                capture_status,
                playback_status,
                capture_delay,
                playback_delay,
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

// Prefills playback with silence, links the pair if not already linked, and starts capture
// (which starts playback too via kernel link-group propagation) or starts both explicitly if
// unlinked. Call only when both PCMs are Prepared.
//
// snd_pcm_link() only synchronizes PCMs sharing one card's hardware trigger, so it can fail
// (e.g. an `asym` PCM spanning two cards) while both PCMs still open and run fine independently.
// cpal can't verify hardware clock sharing either way, so a failed link doesn't refuse the
// stream: it proceeds unlinked instead of gating on a signal it can't fully trust.
fn begin_duplex_playback(stream: &DuplexStreamInner) -> Result<(), Error> {
    let mut silence = vec![0u8; stream.period_size * stream.playback.frame_size].into_boxed_slice();
    stream.playback.equilibrium.fill(&mut silence);
    for _ in 0..DEFAULT_PERIODS {
        let mut frames_written = 0;
        while frames_written < stream.period_size {
            let n = stream
                .playback
                .handle
                .io_bytes()
                .writei(&silence[frames_written * stream.playback.frame_size..])?;
            frames_written += n;
        }
    }

    if !stream.linked.load(Ordering::Relaxed)
        && stream.capture.handle.link(&stream.playback.handle).is_ok()
    {
        stream.linked.store(true, Ordering::Relaxed);
    }

    stream.capture.handle.start()?;
    if !stream.linked.load(Ordering::Relaxed) {
        stream.playback.handle.start()?;
    }
    Ok(())
}

// On xrun, drop() and prepare() both PCMs. When linked this is one call each: DROP and PREPARE
// propagate through the kernel link group the same way START does (snd_pcm_action_group()
// applies to every substream in the group, not just the one the ioctl was issued on).
fn recover_duplex(stream: &DuplexStreamInner) -> Result<(), Error> {
    stream.pending_xrun.store(true, Ordering::Relaxed);
    if stream.linked.load(Ordering::Relaxed) {
        stream.capture.handle.drop()?;
        stream.capture.handle.prepare()?;
    } else {
        stream.capture.handle.drop().ok();
        stream.playback.handle.drop().ok();
        stream.capture.handle.prepare()?;
        stream.playback.handle.prepare()?;
    }
    begin_duplex_playback(stream)
}

struct DuplexStreamWorkerContext {
    descriptors: Box<[libc::pollfd]>,
    capture_range: std::ops::Range<usize>,
    playback_range: std::ops::Range<usize>,
    capture_buffer: Box<[u8]>,
    playback_buffer: Box<[u8]>,
    poll_timeout: i32,
}

impl DuplexStreamWorkerContext {
    fn new(
        poll_timeout: &Option<Duration>,
        stream: &DuplexStreamInner,
        rx: &TriggerReceiver,
    ) -> Self {
        let poll_timeout: i32 = if let Some(d) = poll_timeout {
            d.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as i32
        } else {
            POLL_INFINITE
        };

        let capture_buffer =
            vec![0u8; stream.period_size * stream.capture.frame_size].into_boxed_slice();
        let playback_buffer =
            vec![0u8; stream.period_size * stream.playback.frame_size].into_boxed_slice();

        let capture_count = stream.capture.handle.count();
        let playback_count = stream.playback.handle.count();
        let mut descriptors = vec![
            libc::pollfd {
                fd: 0,
                events: 0,
                revents: 0
            };
            1 + capture_count + playback_count
        ]
        .into_boxed_slice();

        descriptors[0] = libc::pollfd {
            fd: rx.0,
            events: libc::POLLIN,
            revents: 0,
        };

        let capture_range = 1..(1 + capture_count);
        let playback_range = capture_range.end..(capture_range.end + playback_count);

        let filled = stream
            .capture
            .handle
            .fill(&mut descriptors[capture_range.clone()])
            .expect("Failed to fill ALSA capture descriptors");
        debug_assert_eq!(filled, capture_count);
        let filled = stream
            .playback
            .handle
            .fill(&mut descriptors[playback_range.clone()])
            .expect("Failed to fill ALSA playback descriptors");
        debug_assert_eq!(filled, playback_count);

        Self {
            descriptors,
            capture_range,
            playback_range,
            capture_buffer,
            playback_buffer,
            poll_timeout,
        }
    }
}

#[expect(clippy::large_enum_variant)]
enum DuplexPoll {
    Pending,
    Ready {
        capture_status: alsa::pcm::Status,
        playback_status: alsa::pcm::Status,
        capture_delay: usize,
        playback_delay: usize,
    },
    Recover,
}

// Neither direction is processed until both have a full period ready, keeping capture and
// playback in lockstep when snd_pcm_link() couldn't tie them together (virtual PCMs like
// default or pulse have no shared substream to link, so this is common on non-hw devices).
// Suspend goes straight to full recovery instead of a soft hardware resume, since duplex
// would need to keep that resume path in sync across two handles.
fn poll_for_duplex_period(
    rx: &TriggerReceiver,
    stream: &DuplexStreamInner,
    ctxt: &mut DuplexStreamWorkerContext,
) -> Result<DuplexPoll, Error> {
    let res = alsa::poll::poll(&mut ctxt.descriptors, ctxt.poll_timeout)?;
    if res == 0 {
        for handle in [&stream.capture.handle, &stream.playback.handle] {
            match handle.state() {
                alsa::pcm::State::Disconnected => {
                    return Err(Error::with_message(
                        ErrorKind::DeviceNotAvailable,
                        "Device disconnected",
                    ));
                }
                alsa::pcm::State::XRun | alsa::pcm::State::Suspended => {
                    stream.pending_xrun.store(true, Ordering::Relaxed);
                    return Ok(DuplexPoll::Recover);
                }
                _ => {}
            }
        }
        return Ok(DuplexPoll::Pending);
    }

    if ctxt.descriptors[0].revents != 0 {
        rx.clear_pipe();
        return Ok(DuplexPoll::Pending);
    }

    let capture_revents = stream
        .capture
        .handle
        .revents(&ctxt.descriptors[ctxt.capture_range.clone()])?;
    let playback_revents = stream
        .playback
        .handle
        .revents(&ctxt.descriptors[ctxt.playback_range.clone()])?;
    if capture_revents.is_empty() && playback_revents.is_empty() {
        return Ok(DuplexPoll::Pending);
    }
    if capture_revents.intersects(alsa::poll::Flags::HUP | alsa::poll::Flags::NVAL)
        || playback_revents.intersects(alsa::poll::Flags::HUP | alsa::poll::Flags::NVAL)
    {
        return Err(Error::with_message(
            ErrorKind::DeviceNotAvailable,
            "Device disconnected",
        ));
    }

    let (capture_avail, capture_delay) = match stream.capture.handle.avail_delay() {
        Err(_) if matches!(stream.capture.handle.state(), alsa::pcm::State::Suspended) => {
            stream.pending_xrun.store(true, Ordering::Relaxed);
            return Ok(DuplexPoll::Recover);
        }
        Err(err) if err.errno() == libc::EPIPE => {
            stream.pending_xrun.store(true, Ordering::Relaxed);
            return Ok(DuplexPoll::Recover);
        }
        res => res,
    }?;
    let (playback_avail, playback_delay) = match stream.playback.handle.avail_delay() {
        Err(_) if matches!(stream.playback.handle.state(), alsa::pcm::State::Suspended) => {
            stream.pending_xrun.store(true, Ordering::Relaxed);
            return Ok(DuplexPoll::Recover);
        }
        Err(err) if err.errno() == libc::EPIPE => {
            stream.pending_xrun.store(true, Ordering::Relaxed);
            return Ok(DuplexPoll::Recover);
        }
        res => res,
    }?;
    if capture_avail < stream.period_size as alsa::pcm::Frames
        || playback_avail < stream.period_size as alsa::pcm::Frames
    {
        return Ok(DuplexPoll::Pending);
    }

    let capture_status =
        status_with_timestamp(&stream.capture.handle, stream.capture.timestamp_mode)?;
    let playback_status =
        status_with_timestamp(&stream.playback.handle, stream.playback.timestamp_mode)?;

    Ok(DuplexPoll::Ready {
        capture_status,
        playback_status,
        capture_delay: capture_delay.max(0) as usize,
        playback_delay: playback_delay.max(0) as usize,
    })
}

#[expect(clippy::too_many_arguments)]
fn process_duplex(
    stream: &DuplexStreamInner,
    capture_buffer: &mut [u8],
    playback_buffer: &mut [u8],
    capture_status: alsa::pcm::Status,
    playback_status: alsa::pcm::Status,
    capture_delay: usize,
    playback_delay: usize,
    data_callback: &mut (dyn FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static),
) -> Result<(), Error> {
    let mut frames_read = 0;
    while frames_read < stream.period_size {
        match stream
            .capture
            .handle
            .io_bytes()
            .readi(&mut capture_buffer[frames_read * stream.capture.frame_size..])
        {
            Ok(n) => frames_read += n,
            Err(err) if err.errno() == libc::EAGAIN && frames_read == 0 => return Ok(()),
            Err(_) if matches!(stream.capture.handle.state(), alsa::pcm::State::Suspended) => {
                return recover_duplex(stream);
            }
            Err(err) if err.errno() == libc::EAGAIN || err.errno() == libc::EPIPE => {
                return recover_duplex(stream);
            }
            Err(err) => return Err(err.into()),
        }
    }

    stream.playback.equilibrium.fill(playback_buffer);

    if !stream.control.draining.load(Ordering::Relaxed) {
        let input_ptr = capture_buffer.as_ptr() as *mut ();
        let input_data = unsafe {
            Data::from_parts(
                input_ptr,
                stream.capture.period_samples,
                stream.capture.sample_format,
            )
        };
        let output_ptr = playback_buffer.as_mut_ptr() as *mut ();
        let mut output_data = unsafe {
            Data::from_parts(
                output_ptr,
                stream.playback.period_samples,
                stream.playback.sample_format,
            )
        };

        let capture_instant = callback_instant_for(
            stream.capture.timestamp_mode,
            stream.capture.creation_ts,
            stream.creation_instant,
            &capture_status,
        );
        let capture_delay_duration =
            frames_to_duration(capture_delay as FrameCount, stream.sample_rate);
        let capture_device = capture_instant
            .checked_sub(capture_delay_duration)
            .unwrap_or(StreamInstant::ZERO);

        let playback_instant = callback_instant_for(
            stream.playback.timestamp_mode,
            stream.playback.creation_ts,
            stream.creation_instant,
            &playback_status,
        );
        let playback_delay_duration =
            frames_to_duration(playback_delay as FrameCount, stream.sample_rate);
        let playback_device = playback_instant + playback_delay_duration;

        let xrun = stream.pending_xrun.swap(false, Ordering::Relaxed);
        let info = DuplexCallbackInfo::new(
            CallbackInfo {
                timestamp: StreamTimestamp {
                    callback: capture_instant,
                    device: capture_device,
                },
                xrun,
            },
            CallbackInfo {
                timestamp: StreamTimestamp {
                    callback: playback_instant,
                    device: playback_device,
                },
                xrun,
            },
        );
        data_callback(&input_data, &mut output_data, &info);
    }

    let mut frames_written = 0;
    while frames_written < stream.period_size {
        match stream
            .playback
            .handle
            .io_bytes()
            .writei(&playback_buffer[frames_written * stream.playback.frame_size..])
        {
            Ok(n) => frames_written += n,
            Err(err) if err.errno() == libc::EAGAIN && frames_written == 0 => return Ok(()),
            Err(_) if matches!(stream.playback.handle.state(), alsa::pcm::State::Suspended) => {
                return recover_duplex(stream);
            }
            Err(err) if err.errno() == libc::EAGAIN || err.errno() == libc::EPIPE => {
                return recover_duplex(stream);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}
