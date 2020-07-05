use crate::ChannelCount;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{
    BuildStreamError, Data, DefaultStreamConfigError, DeviceNameError, DevicesError,
    PauseStreamError, PlayStreamError, SampleFormat, SampleRate, StreamConfig, StreamError,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
};

const TEMP_BUFFER_SIZE: usize = 16;
use super::JACK_SAMPLE_FORMAT;
pub struct Stream {
    // TODO: It might be faster to send a message when playing/pausing than to check this every iteration
    playing: Arc<AtomicBool>,
    async_client: jack::AsyncClient<(), LocalProcessHandler>,
    // Port names are stored in order to connect them to other ports in jack automatically
    input_port_names: Vec<String>,
    output_port_names: Vec<String>,
}

impl Stream {
    // TODO: Return error messages
    pub fn new_input<D, E>(
        client: jack::Client,
        channels: ChannelCount,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Stream
    where
        D: FnMut(&Data) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let mut ports = vec![];
        let mut port_names: Vec<String> = vec![];
        // Create ports
        for i in 0..channels {
            let mut port_try = client
                .register_port(&format!("out_{}", i), jack::AudioOut::default());
            match(port_try) {
                Ok(port) => {
                    // Get the port name in order to later connect it automatically
                    if let Ok(port_name) = port.name() {
                        port_names.push(port_name);
                    }
                    // Store the port into a Vec to move to the ProcessHandler
                    ports.push(port);
                },
                Err(e) => {
                    // If port creation failed, send the error back via the error_callback
                    error_callback(BackendSpecificError { e }.into());
                }
            }
        }

        let playing = Arc::new(AtomicBool::new(true));

        let input_process_handler = LocalProcessHandler::new(
            vec![],
            ports,
            SampleRate(client.sample_rate() as u32),
            Some(Arc::new(Mutex::new(Box::new(data_callback)))),
            None,
            playing.clone(),
            client.buffer_size() as usize,
        );

        // TODO: Add notification handler, using the error callback?
        let async_client = client.activate_async((), input_process_handler).unwrap();

        Stream {
            playing,
            async_client,
            input_port_names: port_names,
            output_port_names: vec![],
        }
    }

    pub fn new_output<D, E>(
        client: jack::Client,
        channels: ChannelCount,
        mut data_callback: D,
        mut error_callback: E,
    ) -> Stream
    where
        D: FnMut(&mut Data) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let mut ports = vec![];
        let mut port_names: Vec<String> = vec![];
        // Create ports
        for i in 0..channels {
            let mut port_try = client
                .register_port(&format!("out_{}", i), jack::AudioOut::default());
            match(port_try) {
                Ok(port) => {
                    // Get the port name in order to later connect it automatically
                    if let Ok(port_name) = port.name() {
                        port_names.push(port_name);
                    }
                    // Store the port into a Vec to move to the ProcessHandler
                    ports.push(port);
                },
                Err(e) => {
                    // If port creation failed, send the error back via the error_callback
                    error_callback(BackendSpecificError { e }.into());
                }
            }
        }

        let playing = Arc::new(AtomicBool::new(true));

        let output_process_handler = LocalProcessHandler::new(
            ports,
            vec![],
            SampleRate(client.sample_rate() as u32),
            None,
            Some(Arc::new(Mutex::new(Box::new(data_callback)))),
            playing.clone(),
            client.buffer_size() as usize,
        );

        // TODO: Add notification handler, using the error callback?
        let async_client = client.activate_async((), output_process_handler).unwrap();

        Stream {
            playing,
            async_client,
            input_port_names: vec![],
            output_port_names: port_names,
        }
    }

    /// Connect to the standard system outputs in jack, system:playback_1 and system:playback_2
    /// This has to be done after the client is activated, doing it just after creating the ports doesn't work.
    pub fn connect_to_system_outputs(&mut self) {
        // Get the system ports
        let system_ports = self.async_client.as_client().ports(
            Some("system:playback_.*"),
            None,
            jack::PortFlags::empty(),
        );

        // Connect outputs from this client to the system playback inputs
        for i in 0..self.output_port_names.len() {
            if i >= system_ports.len() {
                break;
            }
            match self
                .async_client
                .as_client()
                .connect_ports_by_name(&self.output_port_names[i], &system_ports[i])
            {
                Ok(_) => (),
                Err(e) => println!("Unable to connect to port with error {}", e),
            }
        }
    }

    /// Connect to the standard system outputs in jack, system:capture_1 and system:capture_2
    /// This has to be done after the client is activated, doing it just after creating the ports doesn't work.
    pub fn connect_to_system_inputs(&mut self) {
        // Get the system ports
        let system_ports = self.async_client.as_client().ports(
            Some("system:capture_.*"),
            None,
            jack::PortFlags::empty(),
        );

        // Connect outputs from this client to the system playback inputs
        for i in 0..self.input_port_names.len() {
            if i >= system_ports.len() {
                break;
            }
            match self
                .async_client
                .as_client()
                .connect_ports_by_name(&system_ports[i], &self.input_port_names[i])
            {
                Ok(_) => (),
                Err(e) => println!("Unable to connect to port with error {}", e),
            }
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.playing.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.playing.store(false, Ordering::SeqCst);
        Ok(())
    }
}

struct LocalProcessHandler {
    /// No new ports are allowed to be created after the creation of the LocalProcessHandler as that would invalidate the buffer sizes
    out_ports: Vec<jack::Port<jack::AudioOut>>,
    in_ports: Vec<jack::Port<jack::AudioIn>>,
    // out_port_buffers: Vec<&mut [f32]>,
    // in_port_buffers: Vec<&[f32]>,
    sample_rate: SampleRate,
    input_data_callback: Option<Arc<Mutex<Box<dyn FnMut(&Data) + Send + 'static>>>>,
    output_data_callback: Option<Arc<Mutex<Box<dyn FnMut(&mut Data) + Send + 'static>>>>,

    temp_input_buffer: Vec<f32>,
    /// The number of frames in the temp_input_buffer i.e. temp_input_buffer.len() / in_ports.len()
    temp_input_buffer_size: usize,
    temp_input_buffer_index: usize,

    // JACK audio samples are 32 bit float (unless you do some custom dark magic)
    temp_output_buffer: Vec<f32>,
    /// The number of frames in the temp_output_buffer
    temp_output_buffer_size: usize,
    temp_output_buffer_index: usize,
    playing: Arc<AtomicBool>,
}

impl LocalProcessHandler {
    fn new(
        out_ports: Vec<jack::Port<jack::AudioOut>>,
        in_ports: Vec<jack::Port<jack::AudioIn>>,
        sample_rate: SampleRate,
        input_data_callback: Option<Arc<Mutex<Box<dyn FnMut(&Data) + Send + 'static>>>>,
        output_data_callback: Option<Arc<Mutex<Box<dyn FnMut(&mut Data) + Send + 'static>>>>,
        playing: Arc<AtomicBool>,
        buffer_size: usize,
    ) -> Self {
        // buffer_size is the maximum number of samples per port JACK can request/provide in a single call
        // If it can be fewer than that per call the temp_input_buffer needs to be the smallest multiple of that.
        let mut temp_input_buffer = vec![0.0; in_ports.len() * buffer_size];

        let mut temp_output_buffer = vec![0.0; out_ports.len() * buffer_size];

        // let out_port_buffers = Vec::with_capacity(out_ports.len());
        // let in_port_buffers = Vec::with_capacity(in_ports.len());

        LocalProcessHandler {
            out_ports,
            in_ports,
            // out_port_buffers,
            // in_port_buffers,
            sample_rate,
            input_data_callback,
            output_data_callback,
            temp_input_buffer,
            temp_input_buffer_size: buffer_size,
            temp_input_buffer_index: 0,
            temp_output_buffer,
            temp_output_buffer_size: buffer_size,
            temp_output_buffer_index: 0,
            playing,
        }
    }
}

fn temp_output_buffer_to_data(temp_output_buffer: &mut Vec<f32>) -> Data {
    let data = temp_output_buffer.as_mut_ptr() as *mut ();
    let len = temp_output_buffer.len();
    let data = unsafe { Data::from_parts(data, len, JACK_SAMPLE_FORMAT) };
    data
}

fn temp_input_buffer_to_data(temp_input_buffer: &mut Vec<f32>, total_buffer_size: usize) -> Data {
    let slice = &temp_input_buffer[0..total_buffer_size];
    let data = slice.as_ptr() as *mut ();
    let len = total_buffer_size;
    let data = unsafe { Data::from_parts(data, len, JACK_SAMPLE_FORMAT) };
    data
}

impl jack::ProcessHandler for LocalProcessHandler {
    fn process(&mut self, _: &jack::Client, process_scope: &jack::ProcessScope) -> jack::Control {
        if !self.playing.load(Ordering::SeqCst) {
            return jack::Control::Continue;
        }

        let current_buffer_size = process_scope.n_frames() as usize;

        if let Some(input_callback_mutex) = &mut self.input_data_callback {
            // There is an input callback
            let input_callback = &mut *input_callback_mutex.lock().unwrap();
            // Let's get the data from the input ports and run the callback

            let num_in_channels = self.in_ports.len();

            // Read the data from the input ports into the temporary buffer
            // Go through every channel and store its data in the temporary input buffer
            for ch_ix in 0..num_in_channels {
                let input_channel = &self.in_ports[ch_ix].as_slice(process_scope);
                for i in 0..current_buffer_size {
                    self.temp_input_buffer[ch_ix + i * num_in_channels] = input_channel[i];
                }
            }
            // Create a slice of exactly current_buffer_size frames
            let data = temp_input_buffer_to_data(&mut self.temp_input_buffer, current_buffer_size*num_in_channels);
            input_callback(&data);
        }

        if let Some(output_callback_mutex) = &mut self.output_data_callback {
            // Nothing else should ever lock this Mutex
            let output_callback = &mut *output_callback_mutex.lock().unwrap();

            // Get the mutable slices for each output port buffer
            // for i in 0..self.out_ports.len() {
            //     self.out_port_buffers[i] = self.out_ports[i].as_mut_slice(process_scope);
            // }

            let num_out_channels = self.out_ports.len();

            // Run the output callback on the temporary output buffer until we have filled the output ports
            // JACK ports each provide a mutable slice to be filled with samples whereas CPAL uses interleaved 
            // channels. The formats therefore have to be bridged.
            for i in 0..current_buffer_size {
                if self.temp_output_buffer_index == self.temp_output_buffer_size {
                    // Get new samples if the temporary buffer is depleted
                    let mut data = temp_output_buffer_to_data(&mut self.temp_output_buffer);
                    output_callback(&mut data);
                    self.temp_output_buffer_index = 0;
                }
                // Write the interleaved samples e.g. [l0, r0, l1, r1, ..] to each output buffer
                for ch_ix in 0..num_out_channels {
                    // TODO: This could be very slow, it would be faster to store pointers to these slices, but I don't know how
                    // to avoid lifetime issues and allocation
                    let output_channel = &mut self.out_ports[ch_ix].as_mut_slice(process_scope);
                    output_channel[i] = self.temp_output_buffer
                        [ch_ix + self.temp_output_buffer_index * num_out_channels];
                }
                // Increase the index into the temporary buffer
                self.temp_output_buffer_index += 1;
            }
        }

        // Continue as normal
        jack::Control::Continue
    }
}