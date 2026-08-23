use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use super::JACK_SAMPLE_FORMAT;
use crate::host::try_emit_error;
use crate::{
    CallbackInfo, ChannelCount, Data, DuplexCallbackInfo, Error, ErrorKind, FrameCount, ResultExt,
    Sample, SampleRate, StreamInstant, StreamTimestamp,
    host::{ErrorCallbackArc, emit_error, frames_to_duration},
    traits::StreamTrait,
};

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamState {
    Starting = 0,
    Paused = 1,
    Playing = 2,
}

impl StreamState {
    fn load(atom: &AtomicU8, order: Ordering) -> Self {
        match atom.load(order) {
            1 => Self::Paused,
            2 => Self::Playing,
            _ => Self::Starting,
        }
    }

    fn store(self, atom: &AtomicU8, order: Ordering) {
        atom.store(self as u8, order);
    }
}

pub struct Stream {
    playback_state: Arc<AtomicU8>,
    async_client: jack::AsyncClient<JackNotificationHandler, LocalProcessHandler>,
    // Port names are stored in order to connect them to other ports in jack automatically
    input_port_names: Box<[String]>,
    output_port_names: Box<[String]>,
}

impl Stream {
    pub fn new_input<D, E>(
        client: jack::Client,
        channels: ChannelCount,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (ports, port_names) = register_ports::<jack::AudioIn>(&client, "in", channels)?;
        activate(
            client,
            vec![],
            ports,
            vec![],
            port_names,
            ProcessCallback::Input(Box::new(data_callback)),
            error_callback,
        )
    }

    pub fn new_output<D, E>(
        client: jack::Client,
        channels: ChannelCount,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&mut Data, &CallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (ports, port_names) = register_ports::<jack::AudioOut>(&client, "out", channels)?;
        activate(
            client,
            ports,
            vec![],
            port_names,
            vec![],
            ProcessCallback::Output(Box::new(data_callback)),
            error_callback,
        )
    }

    pub fn new_duplex<D, E>(
        client: jack::Client,
        input_channels: ChannelCount,
        output_channels: ChannelCount,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (in_ports, input_port_names) =
            register_ports::<jack::AudioIn>(&client, "in", input_channels)?;
        let (out_ports, output_port_names) =
            register_ports::<jack::AudioOut>(&client, "out", output_channels)?;
        activate(
            client,
            out_ports,
            in_ports,
            input_port_names,
            output_port_names,
            ProcessCallback::Duplex(Box::new(data_callback)),
            error_callback,
        )
    }

    /// Connects the stream's output ports to as many system playback ports as are available;
    /// must be called after the client is activated. A stream may have more output channels
    /// than physical ports (e.g. feeding a downstream JACK client); the surplus is simply left
    /// unconnected for manual patching.
    ///
    /// Returns `Err` only if an individual port-connection call fails, rolling back any
    /// connections already made so the JACK graph is left unchanged.
    pub fn connect_to_system_outputs(&mut self) -> Result<(), Error> {
        let client = self.async_client.as_client();
        let system_ports = client.ports(Some("system:playback_.*"), None, jack::PortFlags::empty());

        // Connect outputs from this client to the system playback inputs.
        for (i, (our_port, system_port)) in
            self.output_port_names.iter().zip(&system_ports).enumerate()
        {
            if let Err(e) = client.connect_ports_by_name(our_port, system_port) {
                for (prev_our, prev_sys) in
                    self.output_port_names[..i].iter().zip(&system_ports[..i])
                {
                    let _ = client.disconnect_ports_by_name(prev_our, prev_sys);
                }

                return Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    format!("JACK failed to connect port '{our_port}' to '{system_port}': {e}"),
                ));
            }
        }
        Ok(())
    }

    /// Connects the stream's input ports to as many system capture ports as are available; must
    /// be called after the client is activated. A stream may have more input channels than
    /// physical ports (e.g. sourced from an upstream JACK client); the surplus is simply left
    /// unconnected for manual patching.
    ///
    /// Returns `Err` only if an individual port-connection call fails, rolling back any
    /// connections already made so the JACK graph is left unchanged.
    pub fn connect_to_system_inputs(&mut self) -> Result<(), Error> {
        let client = self.async_client.as_client();
        let system_ports = client.ports(Some("system:capture_.*"), None, jack::PortFlags::empty());

        // Connect inputs from system capture ports to this client.
        for (i, (system_port, our_port)) in
            system_ports.iter().zip(&self.input_port_names).enumerate()
        {
            if let Err(e) = client.connect_ports_by_name(system_port, our_port) {
                for (prev_sys, prev_our) in
                    system_ports[..i].iter().zip(&self.input_port_names[..i])
                {
                    let _ = client.disconnect_ports_by_name(prev_sys, prev_our);
                }

                return Err(Error::with_message(
                    ErrorKind::DeviceNotAvailable,
                    format!("JACK failed to connect port '{system_port}' to '{our_port}': {e}"),
                ));
            }
        }
        Ok(())
    }
}

/// Registers `count` ports named `{prefix}_0`, `{prefix}_1`, ... on `client`, returning them
/// alongside the names JACK assigned so callers can later patch them automatically.
fn register_ports<PS>(
    client: &jack::Client,
    prefix: &str,
    count: ChannelCount,
) -> Result<(Vec<jack::Port<PS>>, Vec<String>), Error>
where
    PS: jack::PortSpec + Default,
{
    let mut ports = Vec::with_capacity(count as usize);
    let mut port_names = Vec::with_capacity(count as usize);
    for i in 0..count {
        let port = client
            .register_port(&format!("{prefix}_{i}"), PS::default())
            .context(format!("Failed to register {prefix} port {i}"))?;
        if let Ok(name) = port.name() {
            port_names.push(name);
        }
        ports.push(port);
    }
    Ok((ports, port_names))
}

/// Wires `callback` into a process handler and activates `client`, returning the stream paused.
fn activate<E>(
    client: jack::Client,
    out_ports: Vec<jack::Port<jack::AudioOut>>,
    in_ports: Vec<jack::Port<jack::AudioIn>>,
    input_port_names: Vec<String>,
    output_port_names: Vec<String>,
    callback: ProcessCallback,
    error_callback: E,
) -> Result<Stream, Error>
where
    E: FnMut(Error) + Send + 'static,
{
    let playback_state = Arc::new(AtomicU8::new(StreamState::Starting as u8));
    let pending_xrun = Arc::new(AtomicBool::new(false));
    let error_callback_ptr: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));

    let process_handler = LocalProcessHandler::new(
        out_ports,
        in_ports,
        client.sample_rate(),
        client.buffer_size() as usize,
        callback,
        playback_state.clone(),
        pending_xrun.clone(),
        error_callback_ptr.clone(),
    );

    let notification_handler = JackNotificationHandler::new(
        error_callback_ptr,
        playback_state.clone(),
        client.sample_rate() as jack::Frames,
        pending_xrun,
    );

    let async_client = client
        .activate_async(notification_handler, process_handler)
        .context("Failed to activate client")?;

    StreamState::Paused.store(&playback_state, Ordering::Relaxed);
    Ok(Stream {
        playback_state,
        async_client,
        input_port_names: input_port_names.into_boxed_slice(),
        output_port_names: output_port_names.into_boxed_slice(),
    })
}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        StreamState::Playing.store(&self.playback_state, Ordering::Relaxed);
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        StreamState::Paused.store(&self.playback_state, Ordering::Relaxed);
        Ok(())
    }

    fn stop(&self, timeout: Option<std::time::Duration>) -> Result<(), Error> {
        StreamState::Paused.store(&self.playback_state, Ordering::Relaxed);

        let is_output = !self.output_port_names.is_empty();
        if is_output && timeout != Some(std::time::Duration::ZERO) {
            let client = self.async_client.as_client();
            let ports: Vec<_> = self
                .output_port_names
                .iter()
                .filter_map(|name| client.port_by_name(name))
                .collect();
            let latency_frames = hardware_latency_frames(&ports, jack::LatencyType::Playback)
                .unwrap_or(client.buffer_size() as FrameCount);
            let buffered = frames_to_duration(latency_frames, client.sample_rate() as SampleRate);
            let wait = timeout.map_or(buffered, |t| buffered.min(t));
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        micros_to_stream_instant(self.async_client.as_client().time())
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.async_client.as_client().buffer_size() as FrameCount)
    }
}

type InputDataCallback = Box<dyn FnMut(&Data, &CallbackInfo) + Send + 'static>;
type OutputDataCallback = Box<dyn FnMut(&mut Data, &CallbackInfo) + Send + 'static>;
type DuplexDataCallback = Box<dyn FnMut(&Data, &mut Data, &DuplexCallbackInfo) + Send + 'static>;

enum ProcessCallback {
    Input(InputDataCallback),
    Output(OutputDataCallback),
    Duplex(DuplexDataCallback),
}

struct LocalProcessHandler {
    /// No new ports are allowed to be created after the creation of the LocalProcessHandler as that would invalidate the buffer sizes
    out_ports: Vec<jack::Port<jack::AudioOut>>,
    in_ports: Vec<jack::Port<jack::AudioIn>>,

    sample_rate: SampleRate,
    buffer_size: usize,
    callback: ProcessCallback,

    // JACK audio samples are 32-bit float (unless you do some custom dark magic)
    temp_input_buffer: Vec<f32>,
    temp_output_buffer: Vec<f32>,
    playback_state: Arc<AtomicU8>,
    pending_xrun: Arc<AtomicBool>,
    error_callback: ErrorCallbackArc,
    oversized_reported: bool,
    #[cfg(feature = "realtime")]
    rt_checked: bool,
}

impl LocalProcessHandler {
    #[expect(clippy::too_many_arguments)]
    fn new(
        out_ports: Vec<jack::Port<jack::AudioOut>>,
        in_ports: Vec<jack::Port<jack::AudioIn>>,
        sample_rate: SampleRate,
        buffer_size: usize,
        callback: ProcessCallback,
        playback_state: Arc<AtomicU8>,
        pending_xrun: Arc<AtomicBool>,
        error_callback: ErrorCallbackArc,
    ) -> Self {
        let temp_input_buffer = vec![f32::EQUILIBRIUM; in_ports.len() * buffer_size];
        let temp_output_buffer = vec![f32::EQUILIBRIUM; out_ports.len() * buffer_size];

        Self {
            out_ports,
            in_ports,
            sample_rate,
            buffer_size,
            callback,
            temp_input_buffer,
            temp_output_buffer,
            playback_state,
            pending_xrun,
            error_callback,
            oversized_reported: false,
            #[cfg(feature = "realtime")]
            rt_checked: false,
        }
    }
}

#[inline]
fn temp_buffer_to_data(temp_input_buffer: &mut [f32], total_buffer_size: usize) -> Data {
    let slice = &mut temp_input_buffer[0..total_buffer_size];
    let data: *mut () = slice.as_mut_ptr().cast();
    let len = total_buffer_size;
    unsafe { Data::from_parts(data, len, JACK_SAMPLE_FORMAT) }
}

/// Copies this cycle's captured samples from `in_ports` into `temp_input_buffer` and returns an
/// interleaved [`Data`] view over the first `current_frame_count` frames.
#[inline]
fn read_input(
    in_ports: &[jack::Port<jack::AudioIn>],
    temp_input_buffer: &mut [f32],
    process_scope: &jack::ProcessScope,
    current_frame_count: usize,
) -> Data {
    let num_in_channels = in_ports.len();
    for ch_ix in 0..num_in_channels {
        let input_channel = &in_ports[ch_ix].as_slice(process_scope);
        for i in 0..current_frame_count {
            temp_input_buffer[ch_ix + i * num_in_channels] = input_channel[i];
        }
    }
    temp_buffer_to_data(temp_input_buffer, current_frame_count * num_in_channels)
}

/// Silences `temp_output_buffer` for this cycle and returns an interleaved [`Data`] view for the
/// callback to fill.
#[inline]
fn prime_output(
    temp_output_buffer: &mut [f32],
    current_frame_count: usize,
    num_out_channels: usize,
) -> Data {
    let total = current_frame_count * num_out_channels;
    temp_output_buffer[..total].fill(f32::EQUILIBRIUM);
    temp_buffer_to_data(temp_output_buffer, total)
}

/// Deinterlaces `temp_output_buffer` back out to `out_ports` after the callback has filled it.
#[inline]
fn write_output(
    out_ports: &mut [jack::Port<jack::AudioOut>],
    temp_output_buffer: &[f32],
    process_scope: &jack::ProcessScope,
    current_frame_count: usize,
) {
    let num_out_channels = out_ports.len();
    for ch_ix in 0..num_out_channels {
        let output_channel = &mut out_ports[ch_ix].as_mut_slice(process_scope);
        for i in 0..current_frame_count {
            output_channel[i] = temp_output_buffer[ch_ix + i * num_out_channels];
        }
        // A truncated cycle leaves the tail of JACK's port buffer unwritten.
        output_channel[current_frame_count..].fill(f32::EQUILIBRIUM);
    }
}

impl jack::ProcessHandler for LocalProcessHandler {
    fn process(
        &mut self,
        client: &jack::Client,
        process_scope: &jack::ProcessScope,
    ) -> jack::Control {
        if StreamState::load(&self.playback_state, Ordering::Relaxed) != StreamState::Playing {
            // JACK does not zero-fill output port buffers before calling the process handler
            for port in &mut self.out_ports {
                port.as_mut_slice(process_scope).fill(f32::EQUILIBRIUM);
            }
            return jack::Control::Continue;
        }

        #[cfg(feature = "realtime")]
        {
            if !self.rt_checked {
                #[cfg(any(
                    target_os = "linux",
                    target_os = "dragonfly",
                    target_os = "freebsd",
                    target_os = "netbsd",
                ))]
                let denied = {
                    let sched = unsafe { libc::sched_getscheduler(0) };
                    sched != libc::SCHED_FIFO && sched != libc::SCHED_RR
                };

                #[cfg(target_vendor = "apple")]
                let denied = {
                    use mach2::{
                        boolean::boolean_t,
                        kern_return::KERN_SUCCESS,
                        mach_init::mach_thread_self,
                        mach_port::mach_port_deallocate,
                        thread_policy::{
                            THREAD_TIME_CONSTRAINT_POLICY, THREAD_TIME_CONSTRAINT_POLICY_COUNT,
                            thread_policy_get, thread_policy_t,
                            thread_time_constraint_policy_data_t,
                        },
                        traps::mach_task_self,
                    };
                    let mut policy: thread_time_constraint_policy_data_t =
                        unsafe { std::mem::zeroed() };
                    let mut count = THREAD_TIME_CONSTRAINT_POLICY_COUNT;
                    let mut get_default: boolean_t = 0;
                    // SAFETY: mach_thread_self() returns a send right that we must release.
                    let thread_port = unsafe { mach_thread_self() };
                    let kr = unsafe {
                        thread_policy_get(
                            thread_port,
                            THREAD_TIME_CONSTRAINT_POLICY,
                            &mut policy as *mut _ as thread_policy_t,
                            &mut count,
                            &mut get_default,
                        )
                    };
                    unsafe { mach_port_deallocate(mach_task_self(), thread_port) };
                    kr != KERN_SUCCESS || get_default != 0 || policy.period == 0
                };

                #[cfg(target_os = "windows")]
                let denied = {
                    use windows::Win32::System::Threading;
                    let priority =
                        unsafe { Threading::GetThreadPriority(Threading::GetCurrentThread()) };
                    priority < Threading::THREAD_PRIORITY_ABOVE_NORMAL.0
                };

                if denied {
                    if try_emit_error(&self.error_callback, Error::new(ErrorKind::RealtimeDenied))
                        .is_ok()
                    {
                        self.rt_checked = true;
                    }
                } else {
                    self.rt_checked = true;
                }
            }
        }

        // This should be equal to self.buffer_size, but the implementation will work even if
        // it is less. A greater count is truncated to the temp buffers' capacity.
        let requested_frame_count = process_scope.n_frames() as usize;
        let current_frame_count = requested_frame_count.min(self.buffer_size);
        if requested_frame_count > self.buffer_size {
            if !self.oversized_reported {
                let message = format!(
                    "JACK delivered a {requested_frame_count}-frame period, exceeding the configured buffer size of {}; truncated",
                    self.buffer_size
                );
                self.oversized_reported = try_emit_error(
                    &self.error_callback,
                    Error::with_message(ErrorKind::BackendError, message),
                )
                .is_ok();
            }
        } else {
            self.oversized_reported = false;
        }

        // Get timestamp data
        let (current_start_usecs, next_usecs_opt) = match process_scope.cycle_times() {
            Ok(times) => (times.current_usecs, Some(times.next_usecs)),
            Err(_) => {
                // JACK was unable to get the current time information.
                // Fall back to jack_get_time(), which is the same clock source
                // used by now() and cycle_times(), so the epoch stays consistent.
                (client.time(), None)
            }
        };
        let start_cycle_instant = micros_to_stream_instant(current_start_usecs);
        let start_callback_instant = start_cycle_instant
            + frames_to_duration(
                process_scope.frames_since_cycle_start() as FrameCount,
                self.sample_rate,
            );

        let xrun = self.pending_xrun.swap(false, Ordering::Relaxed);

        match &mut self.callback {
            ProcessCallback::Duplex(duplex_callback) => {
                let input_data = read_input(
                    &self.in_ports,
                    &mut self.temp_input_buffer,
                    process_scope,
                    current_frame_count,
                );
                let mut output_data = prime_output(
                    &mut self.temp_output_buffer,
                    current_frame_count,
                    self.out_ports.len(),
                );

                let capture =
                    capture_instant(&self.in_ports, start_cycle_instant, self.sample_rate);
                let playback = playback_instant(
                    &self.out_ports,
                    start_cycle_instant,
                    next_usecs_opt,
                    current_frame_count as FrameCount,
                    self.sample_rate,
                );
                let info = DuplexCallbackInfo::new(
                    CallbackInfo {
                        timestamp: StreamTimestamp {
                            callback: start_callback_instant,
                            device: capture,
                        },
                        xrun,
                    },
                    CallbackInfo {
                        timestamp: StreamTimestamp {
                            callback: start_callback_instant,
                            device: playback,
                        },
                        xrun,
                    },
                );
                duplex_callback(&input_data, &mut output_data, &info);

                write_output(
                    &mut self.out_ports,
                    &self.temp_output_buffer,
                    process_scope,
                    current_frame_count,
                );
            }
            ProcessCallback::Input(input_callback) => {
                let data = read_input(
                    &self.in_ports,
                    &mut self.temp_input_buffer,
                    process_scope,
                    current_frame_count,
                );
                let timestamp = StreamTimestamp {
                    callback: start_callback_instant,
                    device: capture_instant(&self.in_ports, start_cycle_instant, self.sample_rate),
                };
                let info = CallbackInfo { timestamp, xrun };
                input_callback(&data, &info);
            }
            ProcessCallback::Output(output_callback) => {
                let mut data = prime_output(
                    &mut self.temp_output_buffer,
                    current_frame_count,
                    self.out_ports.len(),
                );
                let timestamp = StreamTimestamp {
                    callback: start_callback_instant,
                    device: playback_instant(
                        &self.out_ports,
                        start_cycle_instant,
                        next_usecs_opt,
                        current_frame_count as FrameCount,
                        self.sample_rate,
                    ),
                };
                let info = CallbackInfo { timestamp, xrun };
                output_callback(&mut data, &info);

                write_output(
                    &mut self.out_ports,
                    &self.temp_output_buffer,
                    process_scope,
                    current_frame_count,
                );
            }
        }

        // Continue as normal
        jack::Control::Continue
    }

    fn buffer_size(&mut self, _: &jack::Client, size: jack::Frames) -> jack::Control {
        // The `buffer_size` callback is actually called on the process thread, but
        // it does not need to be suitable for real-time use. Thus we can simply allocate
        // new buffers here. Details: https://github.com/RustAudio/rust-jack/issues/137
        let new_size = size as usize;
        if new_size != self.buffer_size {
            self.buffer_size = new_size;
            self.temp_input_buffer = vec![f32::EQUILIBRIUM; self.in_ports.len() * new_size];
            self.temp_output_buffer = vec![f32::EQUILIBRIUM; self.out_ports.len() * new_size];
        }

        jack::Control::Continue
    }
}

#[inline]
fn micros_to_stream_instant(micros: u64) -> StreamInstant {
    StreamInstant::from_micros(micros)
}

/// Maximum latency, in frames, between `ports` and the hardware for the given direction,
/// or `None` if JACK reports zero.
#[inline]
fn hardware_latency_frames<PS>(
    ports: &[jack::Port<PS>],
    mode: jack::LatencyType,
) -> Option<FrameCount> {
    let frames = ports
        .iter()
        .map(|port| {
            // This reads a cached value and is documented as safe to call from the process callback.
            port.get_latency_range(mode).1
        })
        .max() // conservative: use worst-case latency across all ports
        .unwrap_or(0);
    (frames > 0).then_some(frames as FrameCount)
}

/// When the first frame in this cycle's input buffer was sampled at the ADC, derived from JACK's
/// port-to-hardware capture latency measured from the cycle start.
#[inline]
fn capture_instant(
    in_ports: &[jack::Port<jack::AudioIn>],
    start_cycle_instant: StreamInstant,
    sample_rate: SampleRate,
) -> StreamInstant {
    let latency = hardware_latency_frames(in_ports, jack::LatencyType::Capture)
        .map(|frames| frames_to_duration(frames, sample_rate))
        .unwrap_or_default();
    start_cycle_instant
        .checked_sub(latency)
        .unwrap_or(StreamInstant::ZERO)
}

/// When the first frame written this cycle reaches the DAC, derived from JACK's port-to-hardware
/// playback latency, or the cycle's hardware deadline if JACK reports no latency.
#[inline]
fn playback_instant(
    out_ports: &[jack::Port<jack::AudioOut>],
    start_cycle_instant: StreamInstant,
    next_usecs_opt: Option<u64>,
    current_frame_count: FrameCount,
    sample_rate: SampleRate,
) -> StreamInstant {
    match hardware_latency_frames(out_ports, jack::LatencyType::Playback) {
        // Prefer JACK's port-to-hardware latency, measured from the cycle start.
        Some(frames) => start_cycle_instant + frames_to_duration(frames, sample_rate),
        // When no latency is reported, fall back to next_usecs, the hardware deadline for this
        // cycle.
        None => match next_usecs_opt {
            Some(next_usecs) => micros_to_stream_instant(next_usecs),
            // Fallback to one buffer ahead if that is unavailable too.
            None => start_cycle_instant + frames_to_duration(current_frame_count, sample_rate),
        },
    }
}

/// Receives notifications from the JACK server on JACK's notification thread (single-threaded).
struct JackNotificationHandler {
    error_callback_ptr: ErrorCallbackArc,
    playback_state: Arc<AtomicU8>,
    configured_sample_rate: jack::Frames,
    pending_xrun: Arc<AtomicBool>,
}

impl JackNotificationHandler {
    pub fn new(
        error_callback_ptr: ErrorCallbackArc,
        playback_state: Arc<AtomicU8>,
        configured_sample_rate: jack::Frames,
        pending_xrun: Arc<AtomicBool>,
    ) -> Self {
        JackNotificationHandler {
            error_callback_ptr,
            playback_state,
            configured_sample_rate,
            pending_xrun,
        }
    }
}

impl jack::NotificationHandler for JackNotificationHandler {
    unsafe fn shutdown(&mut self, _status: jack::ClientStatus, reason: &str) {
        if StreamState::load(&self.playback_state, Ordering::Relaxed) == StreamState::Starting {
            return;
        }
        emit_error(
            &self.error_callback_ptr,
            Error::with_message(
                ErrorKind::DeviceNotAvailable,
                format!("JACK server shut down: {reason}"),
            ),
        );
    }

    fn sample_rate(&mut self, _: &jack::Client, srate: jack::Frames) -> jack::Control {
        if srate == self.configured_sample_rate {
            // One of these notifications is sent every time a client is started.
            return jack::Control::Continue;
        }
        if StreamState::load(&self.playback_state, Ordering::Relaxed) != StreamState::Starting {
            emit_error(
                &self.error_callback_ptr,
                Error::with_message(
                    ErrorKind::StreamInvalidated,
                    format!("JACK server changed sample rate to {srate} Hz"),
                ),
            );
        }
        jack::Control::Quit
    }

    fn xrun(&mut self, _: &jack::Client) -> jack::Control {
        if StreamState::load(&self.playback_state, Ordering::Relaxed) == StreamState::Playing {
            self.pending_xrun.store(true, Ordering::Relaxed);
        }
        jack::Control::Continue
    }
}
