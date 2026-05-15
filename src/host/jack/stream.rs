use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};

use super::JACK_SAMPLE_FORMAT;
use crate::{
    host::{emit_error, frames_to_duration, try_emit_error, ErrorCallbackArc},
    traits::StreamTrait,
    ChannelCount, Data, Error, ErrorKind, FrameCount, InputCallbackInfo, InputStreamTimestamp,
    OutputCallbackInfo, OutputStreamTimestamp, ResultExt, Sample, SampleRate, StreamInstant,
};

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum StreamState {
    #[default]
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
    state: Arc<AtomicU8>,
    async_client: jack::AsyncClient<JackNotificationHandler, LocalProcessHandler>,
    // Port names are stored in order to connect them to other ports in jack automatically
    input_port_names: Vec<String>,
    output_port_names: Vec<String>,
}

// Compile-time assertion that Stream is Send and Sync
crate::assert_stream_send!(Stream);
crate::assert_stream_sync!(Stream);

impl Stream {
    pub fn new_input<D, E>(
        client: jack::Client,
        channels: ChannelCount,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let mut ports = vec![];
        let mut port_names: Vec<String> = vec![];
        for i in 0..channels {
            let port = client
                .register_port(&format!("in_{}", i), jack::AudioIn::default())
                .context(format!("failed to register input port {i}"))?;
            if let Ok(port_name) = port.name() {
                port_names.push(port_name);
            }
            ports.push(port);
        }

        let state = Arc::new(AtomicU8::new(StreamState::Starting as u8));
        let error_callback_ptr: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));

        let input_process_handler = LocalProcessHandler::new(
            vec![],
            ports,
            client.sample_rate(),
            client.buffer_size() as usize,
            Some(Box::new(data_callback)),
            None,
            state.clone(),
            #[cfg(feature = "realtime")]
            error_callback_ptr.clone(),
        );

        let notification_handler = JackNotificationHandler::new(
            error_callback_ptr,
            state.clone(),
            client.sample_rate() as jack::Frames,
        );

        let async_client = client
            .activate_async(notification_handler, input_process_handler)
            .context("failed to activate JACK client")?;

        StreamState::Paused.store(&state, Ordering::Release);
        Ok(Self {
            state,
            async_client,
            input_port_names: port_names,
            output_port_names: vec![],
        })
    }

    pub fn new_output<D, E>(
        client: jack::Client,
        channels: ChannelCount,
        data_callback: D,
        error_callback: E,
    ) -> Result<Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let mut ports = vec![];
        let mut port_names: Vec<String> = vec![];
        for i in 0..channels {
            let port = client
                .register_port(&format!("out_{}", i), jack::AudioOut::default())
                .context(format!("failed to register output port {i}"))?;
            if let Ok(port_name) = port.name() {
                port_names.push(port_name);
            }
            ports.push(port);
        }

        let state = Arc::new(AtomicU8::new(StreamState::Starting as u8));
        let error_callback_ptr: ErrorCallbackArc = Arc::new(Mutex::new(error_callback));

        let output_process_handler = LocalProcessHandler::new(
            ports,
            vec![],
            client.sample_rate(),
            client.buffer_size() as usize,
            None,
            Some(Box::new(data_callback)),
            state.clone(),
            #[cfg(feature = "realtime")]
            error_callback_ptr.clone(),
        );

        let notification_handler = JackNotificationHandler::new(
            error_callback_ptr,
            state.clone(),
            client.sample_rate() as jack::Frames,
        );

        let async_client = client
            .activate_async(notification_handler, output_process_handler)
            .context("failed to activate JACK client")?;

        StreamState::Paused.store(&state, Ordering::Release);
        Ok(Self {
            state,
            async_client,
            input_port_names: vec![],
            output_port_names: port_names,
        })
    }

    /// Connect stream output ports to the standard JACK system playback ports.
    /// Must be called after the client is activated.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if every stream channel was connected to a system playback port.
    /// - `Err` if there are fewer system playback ports than stream channels, or if any
    ///   individual port-connection call fails.
    ///
    /// On error, connections that were made before the failure are rolled back on a best-effort
    /// basis so the JACK graph is left unchanged.
    pub fn connect_to_system_outputs(&mut self) -> Result<(), Error> {
        let client = self.async_client.as_client();
        let system_ports = client.ports(Some("system:playback_.*"), None, jack::PortFlags::empty());

        let n_our = self.output_port_names.len();
        let n_sys = system_ports.len();
        if n_sys < n_our {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "JACK: only {n_sys} system playback port(s) available, but the stream has {n_our} output channel(s)"
                ),
            ));
        }

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

    /// Connect stream input ports to the standard JACK system capture ports.
    /// Must be called after the client is activated.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if every stream channel was connected to a system capture port.
    /// - `Err` if there are fewer system capture ports than stream channels, or if any individual
    ///   port-connection call fails.
    ///
    /// On error, connections that were made before the failure are rolled back on a best-effort
    /// basis so the JACK graph is left unchanged.
    pub fn connect_to_system_inputs(&mut self) -> Result<(), Error> {
        let client = self.async_client.as_client();
        let system_ports = client.ports(Some("system:capture_.*"), None, jack::PortFlags::empty());

        let n_our = self.input_port_names.len();
        let n_sys = system_ports.len();
        if n_sys < n_our {
            return Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                format!(
                    "JACK: only {n_sys} system capture port(s) available, but the stream has {n_our} input channel(s)"
                ),
            ));
        }

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

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), Error> {
        StreamState::Playing.store(&self.state, Ordering::Relaxed);
        Ok(())
    }

    fn pause(&self) -> Result<(), Error> {
        StreamState::Paused.store(&self.state, Ordering::Relaxed);
        Ok(())
    }

    fn now(&self) -> StreamInstant {
        micros_to_stream_instant(self.async_client.as_client().time())
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Ok(self.async_client.as_client().buffer_size() as FrameCount)
    }
}

type InputDataCallback = Box<dyn FnMut(&Data, &InputCallbackInfo) + Send + 'static>;
type OutputDataCallback = Box<dyn FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static>;

struct LocalProcessHandler {
    /// No new ports are allowed to be created after the creation of the LocalProcessHandler as that would invalidate the buffer sizes
    out_ports: Vec<jack::Port<jack::AudioOut>>,
    in_ports: Vec<jack::Port<jack::AudioIn>>,

    sample_rate: SampleRate,
    buffer_size: usize,
    input_data_callback: Option<InputDataCallback>,
    output_data_callback: Option<OutputDataCallback>,

    // JACK audio samples are 32-bit float (unless you do some custom dark magic)
    temp_input_buffer: Vec<f32>,
    temp_output_buffer: Vec<f32>,
    state: Arc<AtomicU8>,
    #[cfg(feature = "realtime")]
    error_callback: ErrorCallbackArc,
    #[cfg(feature = "realtime")]
    rt_checked: bool,
}

impl LocalProcessHandler {
    #[allow(clippy::too_many_arguments)]
    fn new(
        out_ports: Vec<jack::Port<jack::AudioOut>>,
        in_ports: Vec<jack::Port<jack::AudioIn>>,
        sample_rate: SampleRate,
        buffer_size: usize,
        input_data_callback: Option<InputDataCallback>,
        output_data_callback: Option<OutputDataCallback>,
        state: Arc<AtomicU8>,
        #[cfg(feature = "realtime")] error_callback: ErrorCallbackArc,
    ) -> Self {
        let temp_input_buffer = vec![0.0; in_ports.len() * buffer_size];
        let temp_output_buffer = vec![0.0; out_ports.len() * buffer_size];

        Self {
            out_ports,
            in_ports,
            sample_rate,
            buffer_size,
            input_data_callback,
            output_data_callback,
            temp_input_buffer,
            temp_output_buffer,
            state,
            #[cfg(feature = "realtime")]
            error_callback,
            #[cfg(feature = "realtime")]
            rt_checked: false,
        }
    }
}

fn temp_buffer_to_data(temp_input_buffer: &mut [f32], total_buffer_size: usize) -> Data {
    let slice = &mut temp_input_buffer[0..total_buffer_size];
    let data: *mut () = slice.as_mut_ptr().cast();
    let len = total_buffer_size;
    unsafe { Data::from_parts(data, len, JACK_SAMPLE_FORMAT) }
}

impl jack::ProcessHandler for LocalProcessHandler {
    fn process(
        &mut self,
        client: &jack::Client,
        process_scope: &jack::ProcessScope,
    ) -> jack::Control {
        if StreamState::load(&self.state, Ordering::Relaxed) != StreamState::Playing {
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
                            thread_policy_get, thread_policy_t,
                            thread_time_constraint_policy_data_t, THREAD_TIME_CONSTRAINT_POLICY,
                            THREAD_TIME_CONSTRAINT_POLICY_COUNT,
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

        // This should be equal to self.buffer_size, but the implementation will
        // work even if it is less. Will panic in `temp_buffer_to_data` if greater.
        let current_frame_count = process_scope.n_frames() as usize;

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

        if let Some(input_callback) = &mut self.input_data_callback {
            // Let's get the data from the input ports and run the callback

            let num_in_channels = self.in_ports.len();

            // Read the data from the input ports into the temporary buffer
            // Go through every channel and store its data in the temporary input buffer
            for ch_ix in 0..num_in_channels {
                let input_channel = &self.in_ports[ch_ix].as_slice(process_scope);
                for i in 0..current_frame_count {
                    self.temp_input_buffer[ch_ix + i * num_in_channels] = input_channel[i];
                }
            }
            // Create a slice of exactly current_frame_count frames
            let data = temp_buffer_to_data(
                &mut self.temp_input_buffer,
                current_frame_count * num_in_channels,
            );
            // Create timestamp
            let callback = start_callback_instant;
            // Input data was made available at the start of the cycle (current_usecs).
            let capture = start_cycle_instant;
            let timestamp = InputStreamTimestamp { callback, capture };
            let info = InputCallbackInfo { timestamp };
            input_callback(&data, &info);
        }

        if let Some(output_callback) = &mut self.output_data_callback {
            let num_out_channels = self.out_ports.len();

            let total = current_frame_count * num_out_channels;
            self.temp_output_buffer[..total].fill(f32::EQUILIBRIUM);

            // Create a slice of exactly current_frame_count frames
            let mut data = temp_buffer_to_data(&mut self.temp_output_buffer, total);
            // Create timestamp
            let callback = start_callback_instant;
            // Use next_usecs (the hardware deadline for this cycle) when available; it is the
            // exact instant at which the last sample written here will be consumed by the device.
            let playback = match next_usecs_opt {
                Some(next_usecs) => micros_to_stream_instant(next_usecs),
                None => {
                    start_cycle_instant
                        + frames_to_duration(current_frame_count as FrameCount, self.sample_rate)
                }
            };
            let timestamp = OutputStreamTimestamp { callback, playback };
            let info = OutputCallbackInfo { timestamp };
            output_callback(&mut data, &info);

            // Deinterlace
            for ch_ix in 0..num_out_channels {
                let output_channel = &mut self.out_ports[ch_ix].as_mut_slice(process_scope);
                for i in 0..current_frame_count {
                    output_channel[i] = self.temp_output_buffer[ch_ix + i * num_out_channels];
                }
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
            self.temp_input_buffer = vec![0.0; self.in_ports.len() * new_size];
            self.temp_output_buffer = vec![0.0; self.out_ports.len() * new_size];
        }

        jack::Control::Continue
    }
}

#[inline]
fn micros_to_stream_instant(micros: u64) -> StreamInstant {
    StreamInstant::from_micros(micros)
}

/// Receives notifications from the JACK server on JACK's notification thread (single-threaded).
struct JackNotificationHandler {
    error_callback_ptr: ErrorCallbackArc,
    state: Arc<AtomicU8>,
    configured_sample_rate: jack::Frames,
}

impl JackNotificationHandler {
    pub fn new(
        error_callback_ptr: ErrorCallbackArc,
        state: Arc<AtomicU8>,
        configured_sample_rate: jack::Frames,
    ) -> Self {
        JackNotificationHandler {
            error_callback_ptr,
            state,
            configured_sample_rate,
        }
    }
}

impl jack::NotificationHandler for JackNotificationHandler {
    unsafe fn shutdown(&mut self, _status: jack::ClientStatus, reason: &str) {
        if StreamState::load(&self.state, Ordering::Acquire) == StreamState::Starting {
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
        if StreamState::load(&self.state, Ordering::Acquire) != StreamState::Starting {
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
        if StreamState::load(&self.state, Ordering::Acquire) != StreamState::Starting {
            let _ = try_emit_error(
                &self.error_callback_ptr,
                Error::with_message(ErrorKind::Xrun, "JACK xrun detected"),
            );
        }
        jack::Control::Continue
    }
}
