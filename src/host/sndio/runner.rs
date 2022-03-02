use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use super::{backend_specific_error, InnerState};

use crate::{
    Data, FrameCount, InputCallbackInfo, InputStreamTimestamp, OutputCallbackInfo,
    OutputStreamTimestamp, SampleFormat, StreamInstant,
};

/// The runner thread handles playing and/or recording
pub(super) fn runner(inner_state_arc: Arc<Mutex<InnerState>>) {
    let buffer_size: FrameCount;
    let start_time: Instant;
    let latency: Duration;
    let mut clear_output_buf_needed = false;
    let (wakeup_sender, wakeup_receiver) = mpsc::channel();
    {
        let mut inner_state = inner_state_arc.lock().unwrap();
        match *inner_state {
            InnerState::Init { .. } | InnerState::Opened { .. } => {
                // Unlikely error state
                inner_state.error(backend_specific_error(
                    "inner state should be InnerState::Running",
                ));
                return;
            }
            InnerState::Running {
                wakeup_sender: ref mut wakeup_sender_,
                buffer_size: ref buffer_size_,
                ref par,
                ..
            } => {
                *wakeup_sender_ = Some(wakeup_sender);

                buffer_size = *buffer_size_;
                if buffer_size == 0 {
                    // Probably unreachable
                    inner_state.error(backend_specific_error("could not determine buffer size"));
                    return;
                }

                latency = Duration::from_secs(1) * buffer_size / par.rate;
            }
        }

        if let Err(err) = inner_state.start() {
            inner_state.error(err);
            return;
        }

        start_time = Instant::now();
    }

    // TODO: this is converting a FrameCount to a number of samples; invalid for stereo!
    let mut output_buf = [0i16].repeat(buffer_size as usize); // Allocate buffer of correct size
    let mut input_buf = [0i16].repeat(buffer_size as usize); // Allocate buffer of correct size
    let mut output_data = unsafe {
        Data::from_parts(
            output_buf.as_mut_ptr() as *mut (),
            output_buf.len(),
            SampleFormat::I16,
        )
    };
    let input_data = unsafe {
        Data::from_parts(
            input_buf.as_mut_ptr() as *mut (),
            input_buf.len(),
            SampleFormat::I16,
        )
    };
    let data_byte_size = output_data.len * output_data.sample_format.sample_size();

    let mut output_offset_bytes_into_buf: u64 = 0; // Byte offset in output buf to sio_write
    let mut input_offset_bytes_into_buf: u64 = 0; // Byte offset in input buf to sio_read
    let mut paused = false;
    loop {
        // See if shutdown requested in inner_state.status; if so, break
        let mut nfds;
        let mut pollfds: Vec<libc::pollfd>;
        {
            let mut inner_state = inner_state_arc.lock().unwrap();
            // If there's nothing to do, wait until that's no longer the case.
            match *inner_state {
                InnerState::Init { .. } | InnerState::Opened { .. } => {
                    // Unlikely error state
                    inner_state.error(backend_specific_error(
                        "inner state should be InnerState::Running",
                    ));
                    break;
                }
                _ => {}
            }

            if !inner_state.has_streams() {
                if !paused {
                    if let Err(_) = inner_state.stop() {
                        // No callbacks to error with
                        break;
                    }
                }
                paused = true;
                while let Ok(_) = wakeup_receiver.try_recv() {} // While the lock is still held, drain the channel.

                // Unlock to prevent deadlock
                drop(inner_state);

                // Block until a callback has been added; unwrap OK because the sender is in the
                // Arc so it won't get dropped while this thread is running.
                wakeup_receiver.recv().unwrap();
            }
        }

        // If there no Streams and no Device then there is nothing to do -- exit. Note: this is
        // only correct if there are no Weak references to this InnerState anywhere.
        if Arc::strong_count(&inner_state_arc) == 1 {
            break;
        }

        {
            let mut inner_state = inner_state_arc.lock().unwrap();
            if paused {
                match *inner_state {
                    InnerState::Init { .. } | InnerState::Opened { .. } => {
                        // Unlikely error state
                        inner_state.error(backend_specific_error(
                            "inner state should be InnerState::Running",
                        ));
                        break;
                    }
                    InnerState::Running { .. } => {
                        if !inner_state.has_streams() {
                            // Spurious wakeup
                            continue;
                        }
                    }
                }

                if let Err(err) = inner_state.start() {
                    inner_state.error(backend_specific_error(format!(
                        "failed to unpause after new Stream created: {:?}",
                        err
                    )));
                    break;
                }
                paused = false;
            }
            match *inner_state {
                InnerState::Init { .. } | InnerState::Opened { .. } => {
                    // Unlikely error state
                    inner_state.error(backend_specific_error(
                        "inner state should be InnerState::Running",
                    ));
                    break;
                }
                InnerState::Running { ref mut hdl, .. } => {
                    nfds = unsafe { sndio_sys::sio_nfds(hdl.0) };
                    if nfds <= 0 {
                        inner_state.error(backend_specific_error(format!(
                            "cannot allocate {} pollfd structs",
                            nfds
                        )));
                        break;
                    }
                    pollfds = [libc::pollfd {
                        fd: 0,
                        events: 0,
                        revents: 0,
                    }]
                    .repeat(nfds as usize);

                    // Populate pollfd structs with sndio_sys::sio_pollfd
                    nfds = unsafe {
                        sndio_sys::sio_pollfd(
                            hdl.0,
                            pollfds.as_mut_ptr(),
                            (libc::POLLOUT | libc::POLLIN) as i32,
                        )
                    };
                    if nfds <= 0 || nfds > pollfds.len() as i32 {
                        inner_state.error(backend_specific_error(format!(
                            "invalid pollfd count from sio_pollfd: {}",
                            nfds
                        )));
                        break;
                    }
                }
            }
        }

        // Poll (block until ready to write)
        let status = unsafe { libc::poll(pollfds.as_mut_ptr(), nfds as u32, -1) };
        if status < 0 {
            let mut inner_state = inner_state_arc.lock().unwrap();
            inner_state.error(backend_specific_error(format!(
                "poll failed: returned {}",
                status
            )));
            break;
        }

        let revents;
        {
            let mut inner_state = inner_state_arc.lock().unwrap();
            match *inner_state {
                InnerState::Init { .. } | InnerState::Opened { .. } => {
                    // Unlikely error state
                    inner_state.error(backend_specific_error(
                        "inner state should be InnerState::Running",
                    ));
                    break;
                }
                InnerState::Running { ref mut hdl, .. } => {
                    revents = unsafe { sndio_sys::sio_revents(hdl.0, pollfds.as_mut_ptr()) } as i16;
                }
            }
            if revents & libc::POLLHUP != 0 {
                inner_state.error(backend_specific_error("device disappeared"));
                break;
            }
        }

        if revents & (libc::POLLOUT | libc::POLLIN) == 0 {
            continue;
        }

        let elapsed = Instant::now().duration_since(start_time);
        if revents & libc::POLLOUT != 0 {
            // At this point we know data can be written
            let mut output_callback_info = OutputCallbackInfo {
                timestamp: OutputStreamTimestamp {
                    callback: StreamInstant::new(
                        elapsed.as_secs() as i64,
                        elapsed.as_nanos() as u32,
                    ),
                    playback: StreamInstant::new(0, 0), // Set below
                },
            };
            output_callback_info.timestamp.playback = output_callback_info
                .timestamp
                .callback
                .add(latency)
                .unwrap(); // TODO: figure out if overflow can happen

            {
                let mut inner_state = inner_state_arc.lock().unwrap();

                let bytes_written;
                match *inner_state {
                    InnerState::Init { .. } | InnerState::Opened { .. } => {
                        // Unlikely error state
                        inner_state.error(backend_specific_error(
                            "inner state should be InnerState::Running",
                        ));
                        break;
                    }
                    InnerState::Running {
                        ref mut hdl,
                        ref mut output_callbacks,
                        ..
                    } => {
                        if output_offset_bytes_into_buf == 0 {
                            // The whole output buffer has been written (or this is the first time). Fill it.
                            if output_callbacks.empty() {
                                if clear_output_buf_needed {
                                    // There is probably nonzero data in the buffer from previous output
                                    // Streams. Zero it out.
                                    for sample in output_buf.iter_mut() {
                                        *sample = 0;
                                    }
                                    clear_output_buf_needed = false;
                                }
                            } else {
                                for cbs in output_callbacks.store.values_mut() {
                                    // Really we shouldn't have more than one output callback as they are
                                    // stepping on each others' data.
                                    // TODO: perhaps we should not call these callbacks while holding the lock
                                    (cbs.data_callback)(&mut output_data, &output_callback_info);
                                }
                                clear_output_buf_needed = true;
                            }
                        }

                        // unwrap OK because .open was called
                        bytes_written = unsafe {
                            sndio_sys::sio_write(
                                hdl.0,
                                (output_data.data as *const u8)
                                    .add(output_offset_bytes_into_buf as usize)
                                    as *const _,
                                data_byte_size as u64 - output_offset_bytes_into_buf,
                            )
                        };
                    }
                }

                if bytes_written <= 0 {
                    inner_state.error(backend_specific_error("no bytes written; EOF?"));
                    break;
                }

                output_offset_bytes_into_buf += bytes_written;
                if output_offset_bytes_into_buf as usize > data_byte_size {
                    inner_state.error(backend_specific_error("too many bytes written!"));
                    break;
                }

                if output_offset_bytes_into_buf as usize == data_byte_size {
                    // Everything written; need to call data callback again.
                    output_offset_bytes_into_buf = 0;
                };
            }
        }

        if revents & libc::POLLIN != 0 {
            // At this point, we know data can be read
            let mut input_callback_info = InputCallbackInfo {
                timestamp: InputStreamTimestamp {
                    callback: StreamInstant::new(
                        elapsed.as_secs() as i64,
                        elapsed.as_nanos() as u32,
                    ),
                    capture: StreamInstant::new(0, 0),
                },
            };
            if let Some(capture_instant) = input_callback_info.timestamp.callback.sub(latency) {
                input_callback_info.timestamp.capture = capture_instant;
            } else {
                println!("cpal(sndio): Underflow while calculating capture timestamp"); // TODO: is this possible? Handle differently?
                input_callback_info.timestamp.capture = input_callback_info.timestamp.callback;
            }

            {
                let mut inner_state = inner_state_arc.lock().unwrap();

                // unwrap OK because .open was called
                let bytes_read;
                match *inner_state {
                    InnerState::Init { .. } | InnerState::Opened { .. } => {
                        // Unlikely error state
                        inner_state.error(backend_specific_error(
                            "inner state should be InnerState::Running",
                        ));
                        break;
                    }
                    InnerState::Running { ref mut hdl, .. } => {
                        bytes_read = unsafe {
                            sndio_sys::sio_read(
                                hdl.0,
                                (input_data.data as *const u8)
                                    .add(input_offset_bytes_into_buf as usize)
                                    as *mut _,
                                data_byte_size as u64 - input_offset_bytes_into_buf,
                            )
                        }
                    }
                }

                if bytes_read <= 0 {
                    inner_state.error(backend_specific_error("no bytes read; EOF?"));
                    break;
                }

                input_offset_bytes_into_buf += bytes_read;
                if input_offset_bytes_into_buf as usize > data_byte_size {
                    inner_state.error(backend_specific_error("too many bytes read!"));
                    break;
                }

                if input_offset_bytes_into_buf as usize == data_byte_size {
                    // Input buffer is full; need to call data callback again.
                    input_offset_bytes_into_buf = 0;
                };

                if input_offset_bytes_into_buf == 0 {
                    match *inner_state {
                        InnerState::Init { .. } | InnerState::Opened { .. } => {
                            // Unlikely error state
                            inner_state.error(backend_specific_error(
                                "inner state should be InnerState::Running",
                            ));
                            break;
                        }
                        InnerState::Running {
                            ref mut input_callbacks,
                            ..
                        } => {
                            for cbs in input_callbacks.store.values_mut() {
                                // TODO: perhaps we should not call these callbacks while holding the lock
                                (cbs.data_callback)(&input_data, &input_callback_info);
                            }
                        }
                    }
                }
            }
        }
    }

    {
        let mut inner_state = inner_state_arc.lock().unwrap();
        if !paused {
            let _ = inner_state.stop(); // Can't do anything with error since no error callbacks left
        }
        match *inner_state {
            InnerState::Init { .. } | InnerState::Opened { .. } => {
                // anlikely error state but nothing to do with error
                return;
            }
            InnerState::Running {
                ref mut wakeup_sender,
                ref mut thread_spawned,
                ..
            } => {
                *wakeup_sender = None;
                *thread_spawned = false;
            }
        }
    }
}
