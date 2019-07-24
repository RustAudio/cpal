mod enumerate;

pub use self::enumerate::{default_input_device, default_output_device, Devices};

use crate::{
    traits::{DeviceTrait, EventLoopTrait, HostTrait, StreamIdTrait},
    BackendSpecificError, BuildStreamError, ChannelCount, DefaultFormatError, DeviceNameError,
    DevicesError, Format, HostUnavailable, InputBuffer, OutputBuffer, PauseStreamError,
    PlayStreamError, SampleFormat, SampleRate, StreamData, StreamDataResult, StreamError,
    SupportedFormat, SupportedFormatsError, UnknownTypeInputBuffer, UnknownTypeOutputBuffer,
};

use alsa::{
    pcm::{Access, Format as AlsaFormat, HwParams, PCM},
    poll::{PollDescriptors, POLLIN, POLLOUT},
    Direction, ValueOr,
};
use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags},
};
use std::{
    cmp,
    convert::{TryFrom, TryInto},
    iter, mem,
    os::unix::io::RawFd,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{channel, Receiver, Sender},
        Mutex,
    },
    vec::IntoIter as VecIntoIter,
};

pub type SupportedInputFormats = VecIntoIter<SupportedFormat>;
pub type SupportedOutputFormats = VecIntoIter<SupportedFormat>;

/// The default linux and freebsd host type.
#[derive(Debug)]
pub struct Host;

impl Host {
    pub fn new() -> Result<Self, HostUnavailable> {
        Ok(Host)
    }
}

impl HostTrait for Host {
    type Devices = Devices;
    type Device = Device;
    type EventLoop = EventLoop;

    fn is_available() -> bool {
        // Assume ALSA is always available on linux/freebsd. If it isn't then this library will
        // fail when linking.
        true
    }

    fn devices(&self) -> Result<Self::Devices, DevicesError> {
        Devices::new()
    }

    fn default_input_device(&self) -> Option<Self::Device> {
        default_input_device()
    }

    fn default_output_device(&self) -> Option<Self::Device> {
        default_output_device()
    }

    fn event_loop(&self) -> Self::EventLoop {
        EventLoop::new()
    }
}

impl DeviceTrait for Device {
    type SupportedInputFormats = SupportedInputFormats;
    type SupportedOutputFormats = SupportedOutputFormats;

    fn name(&self) -> Result<String, DeviceNameError> {
        Device::name(self)
    }

    fn supported_input_formats(
        &self,
    ) -> Result<Self::SupportedInputFormats, SupportedFormatsError> {
        Device::supported_input_formats(self)
    }

    fn supported_output_formats(
        &self,
    ) -> Result<Self::SupportedOutputFormats, SupportedFormatsError> {
        Device::supported_output_formats(self)
    }

    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_input_format(self)
    }

    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        Device::default_output_format(self)
    }
}

impl EventLoopTrait for EventLoop {
    type Device = Device;
    type StreamId = StreamId;

    fn build_input_stream(
        &self,
        device: &Self::Device,
        format: &Format,
    ) -> Result<Self::StreamId, BuildStreamError> {
        EventLoop::build_input_stream(self, device, format)
    }

    fn build_output_stream(
        &self,
        device: &Self::Device,
        format: &Format,
    ) -> Result<Self::StreamId, BuildStreamError> {
        EventLoop::build_output_stream(self, device, format)
    }

    fn play_stream(&self, stream: Self::StreamId) -> Result<(), PlayStreamError> {
        EventLoop::play_stream(self, stream)
    }

    fn pause_stream(&self, stream: Self::StreamId) -> Result<(), PauseStreamError> {
        EventLoop::pause_stream(self, stream)
    }

    fn destroy_stream(&self, stream: Self::StreamId) {
        EventLoop::destroy_stream(self, stream)
    }

    fn run<F>(&self, callback: F) -> !
    where
        F: FnMut(Self::StreamId, StreamDataResult) + Send,
    {
        EventLoop::run(self, callback)
    }
}

impl StreamIdTrait for StreamId {}

/// This is a wrapper around a pipe, allowing for main processing loop to be notified of incoming
/// events.
struct Trigger {
    // [read fd, write fd]
    read_fd: RawFd,
    write_fd: RawFd,
}

impl Trigger {
    fn new() -> Self {
        let (read_fd, write_fd) = nix::unistd::pipe().unwrap();
        Trigger { read_fd, write_fd }
    }

    fn wakeup(&self) {
        let buf = [0, 0, 0, 0, 0, 0, 0, 1u8];
        let amt = nix::unistd::write(self.write_fd, &buf).unwrap();
        assert_eq!(amt, 8);
    }

    fn clear_pipe(&self) {
        let mut buf = [0u8; 8];
        let amt = nix::unistd::read(self.read_fd, &mut buf).unwrap();
        assert_eq!(amt, 8);
    }
}

impl Drop for Trigger {
    fn drop(&mut self) {
        nix::unistd::close(self.read_fd).unwrap();
        nix::unistd::close(self.write_fd).unwrap();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device(String);

impl Device {
    #[inline]
    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.0.clone())
    }

    fn supported_formats(
        &self,
        direction: Direction,
    ) -> Result<VecIntoIter<SupportedFormat>, SupportedFormatsError> {
        let handle = PCM::new(&self.0, direction, true).map_err(|err| {
            match err.errno() {
                // Map some specific errors
                Some(Errno::ENOENT) | Some(Errno::EBUSY) => {
                    SupportedFormatsError::DeviceNotAvailable
                }
                Some(Errno::EINVAL) => SupportedFormatsError::InvalidArgument,
                _ => unhandled_error(err),
            }
        })?;
        let hw_params =
            HwParams::any(&handle).map_err(unhandled_error::<_, SupportedFormatsError>)?;

        // Test out i16, u16, and f32 data types
        let mut supported_formats = Vec::with_capacity(3);
        if hw_params.test_format(AlsaFormat::s16()) {
            supported_formats.push(SampleFormat::I16);
        }
        if hw_params.test_format(AlsaFormat::u16()) {
            supported_formats.push(SampleFormat::U16);
        }
        if hw_params.test_format(AlsaFormat::float()) {
            supported_formats.push(SampleFormat::F32);
        }
        // TODO potentially more formats could be supported.

        // Get possible sample rates
        let rate_max = hw_params
            .get_rate_max()
            .map_err(unhandled_error::<_, SupportedFormatsError>)?;
        let rate_min = hw_params
            .get_rate_min()
            .map_err(unhandled_error::<_, SupportedFormatsError>)?;

        // TODO This code is copied from the original impl using alsa-sys , and I have seen it
        // elsewhere on the internet. It seems a bit flakey, so if anyone knows how to do it
        // better, then they should improve it.
        // First if min == max, then there is only one rate available
        let sample_rates = if rate_max == rate_min {
            vec![(rate_min, rate_max)]
        // Then, if min and min + 1 are valid, assume all rates are valid in [min, max]
        //} else if hw_params.test_rate(rate_min + 1) {
        //    vec![(rate_min, rate_max)]
        // Otherwise, test some standard rates
        } else {
            const RATES: [u32; 13] = [
                5512, 8000, 11025, 16000, 22050, 32000, 44100, 48000, 64000, 88200, 96000, 176400,
                192000,
            ];
            let mut sample_rates = vec![];
            for &rate in &RATES {
                if rate >= rate_min && rate <= rate_max && hw_params.test_rate(rate) {
                    sample_rates.push((rate, rate));
                }
            }
            sample_rates
        };
        // Get possible number of channels
        let channels_min = hw_params
            .get_channels_min()
            .map_err(unhandled_error::<_, SupportedFormatsError>)?;
        // cap at 32 channels.
        let channels_max = cmp::min(
            hw_params
                .get_channels_max()
                .map_err(unhandled_error::<_, SupportedFormatsError>)?,
            32,
        );
        let supported_channels = (channels_min..=channels_max)
            .filter_map(|v| {
                if hw_params.test_channels(v) {
                    Some(v as ChannelCount)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        // Take the outer product of data_types, channels, and rates.
        let mut output = Vec::with_capacity(
            supported_formats.len() * supported_channels.len() * sample_rates.len(),
        );
        for &data_type in supported_formats.iter() {
            for channels in supported_channels.iter() {
                for &(min_rate, max_rate) in sample_rates.iter() {
                    output.push(SupportedFormat {
                        channels: channels.clone(),
                        min_sample_rate: SampleRate(min_rate as u32),
                        max_sample_rate: SampleRate(max_rate as u32),
                        data_type: data_type,
                    });
                }
            }
        }
        Ok(output.into_iter())
    }

    fn supported_input_formats(&self) -> Result<SupportedInputFormats, SupportedFormatsError> {
        self.supported_formats(Direction::Capture)
    }

    fn supported_output_formats(&self) -> Result<SupportedOutputFormats, SupportedFormatsError> {
        self.supported_formats(Direction::Playback)
    }

    // ALSA does not offer default stream formats, so instead we compare all supported formats by
    // the `SupportedFormat::cmp_default_heuristics` order and select the greatest.
    fn default_format(&self, direction: Direction) -> Result<Format, DefaultFormatError> {
        let mut formats: Vec<_> = match self.supported_formats(direction) {
            Err(SupportedFormatsError::DeviceNotAvailable) => {
                return Err(DefaultFormatError::DeviceNotAvailable);
            }
            Err(SupportedFormatsError::InvalidArgument) => {
                // this happens sometimes when querying for input and output capabilities but
                // the device supports only one
                return Err(DefaultFormatError::StreamTypeNotSupported);
            }
            Err(SupportedFormatsError::BackendSpecific { err }) => {
                return Err(err.into());
            }
            Ok(fmts) => fmts.collect(),
        };

        formats.sort_by(|a, b| a.cmp_default_heuristics(b));

        let handle =
            PCM::new(&self.0, direction, true).map_err(unhandled_error::<_, DefaultFormatError>)?;
        match formats.into_iter().last() {
            Some(f) => {
                let min_r = f.min_sample_rate;
                let max_r = f.max_sample_rate;
                let mut format = f.with_max_sample_rate();
                /*
                const HZ_44100: SampleRate = SampleRate(44_100);
                if min_r <= HZ_44100 && HZ_44100 <= max_r
                    && handle.hw_params_current().map_err(unhandled_error::<_, DefaultFormatError>)?.test_rate(HZ_44100.0)
                {
                    format.sample_rate = HZ_44100;
                }
                */
                Ok(format)
            }
            None => Err(DefaultFormatError::StreamTypeNotSupported),
        }
    }

    fn default_input_format(&self) -> Result<Format, DefaultFormatError> {
        self.default_format(Direction::Capture)
    }

    fn default_output_format(&self) -> Result<Format, DefaultFormatError> {
        self.default_format(Direction::Playback)
    }
}

pub struct EventLoop {
    // Each newly-created stream gets a new ID from this counter. The counter is then incremented.
    next_stream_id: AtomicU64,

    // A trigger that uses a `pipe()` as backend. Signalled whenever a new command is ready, so
    // that `poll()` can wake up and pick the changes.
    pending_command_trigger: Trigger,

    // This field is locked by the `run()` method.
    // The mutex also ensures that only one thread at a time has `run()` running.
    run_context: Mutex<RunContext>,

    // Commands processed by the `run()` method that is currently running.
    commands: Sender<Command>,
}

unsafe impl Send for EventLoop {}

unsafe impl Sync for EventLoop {}

enum Command {
    NewStream(StreamInner),
    PlayStream(StreamId),
    PauseStream(StreamId),
    DestroyStream(StreamId),
}

struct RunContext {
    // Descriptors to wait for. Always contains `pending_command_trigger.read_fd()` as first element.
    // They really belong in the streams, but they must be in a single memory block so we can
    // `poll` them.
    descriptors: Vec<PollFd>,
    // List of streams that are written in `descriptors`.
    streams: Vec<StreamInner>,

    commands: Receiver<Command>,
}

struct StreamInner {
    // The id of the stream.
    id: StreamId,

    // The ALSA channel.
    channel: PCM,

    // The number of pollfds for this stream
    num_descriptors: usize,

    // Format of the samples.
    sample_format: SampleFormat,

    // Number of channels, ie. number of samples per frame.
    num_channels: u16,

    // Number of samples that can fit in the buffer.
    buffer_len: usize,

    // Minimum number of samples to put in the buffer.
    period_len: usize,

    // Whether or not the hardware supports pausing the stream.
    can_pause: bool,

    // Whether or not the sample stream is currently paused.
    is_paused: bool,

    // A file descriptor opened with `eventfd`.
    // It is used to wait for resume signal.
    resume_trigger: Trigger,

    // Lazily allocated buffer that is reused inside the loop.
    // Zero-allocate a new buffer (the fastest way to have zeroed memory) at the first time this is
    // used.
    buffer: Vec<u8>,
}

#[derive(Copy, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId(u64);

enum StreamType {
    Input,
    Output,
}

impl EventLoop {
    #[inline]
    fn new() -> EventLoop {
        let pending_command_trigger = Trigger::new();

        let mut initial_descriptors = vec![];
        reset_descriptors_with_pending_command_trigger(
            &mut initial_descriptors,
            &pending_command_trigger,
        );

        let (tx, rx) = channel();

        let run_context = Mutex::new(RunContext {
            descriptors: initial_descriptors,
            streams: Vec::new(),
            commands: rx,
        });

        EventLoop {
            next_stream_id: AtomicU64::new(0),
            pending_command_trigger: pending_command_trigger,
            run_context,
            commands: tx,
        }
    }

    #[inline]
    fn run<F>(&self, mut callback: F) -> !
    where
        F: FnMut(StreamId, StreamDataResult),
    {
        self.run_inner(&mut callback)
    }

    fn run_inner(&self, callback: &mut dyn FnMut(StreamId, StreamDataResult)) -> ! {
        unsafe {
            let mut run_context = self.run_context.lock().unwrap();
            let run_context = &mut *run_context;

            'stream_loop: loop {
                process_commands(run_context);

                // Rebuild list of descriptors to poll, since streams may have been
                // created/destroyed.
                reset_descriptors_with_pending_command_trigger(
                    &mut run_context.descriptors,
                    &self.pending_command_trigger,
                );
                append_stream_poll_descriptors(run_context);

                // At this point, this should include the command `pending_commands_trigger` along
                // with the poll descriptors for each stream.
                match poll_all_descriptors(&mut run_context.descriptors) {
                    Ok(true) => (),
                    Ok(false) => continue,
                    Err(err) => {
                        for stream in run_context.streams.iter() {
                            let result = Err(err.clone().into());
                            callback(stream.id, result);
                        }
                        run_context.streams.clear();
                        break 'stream_loop;
                    }
                }

                // If the `pending_command_trigger` was signaled, we need to process the comands.
                if run_context.descriptors[0].revents().is_some() {
                    self.pending_command_trigger.clear_pipe();
                }

                // The set of streams that error within the following loop and should be removed.
                let mut streams_to_remove: Vec<(StreamId, StreamError)> = vec![];

                // Iterate over each individual stream/descriptor.
                let mut i_stream = 0;
                let mut i_descriptor: isize = 1;
                while (i_descriptor as usize) < run_context.descriptors.len() {
                    let stream = &mut run_context.streams[i_stream];
                    let desc_start = usize::try_from(i_descriptor).unwrap();
                    let stream_descriptors =
                        &mut run_context.descriptors[desc_start..stream.num_descriptors];

                    // Only go on if this event was a pollout or pollin event.
                    let stream_type = match check_for_pollout_or_pollin(stream, stream_descriptors)
                    {
                        Ok(Some(ty)) => ty,
                        Ok(None) => {
                            i_descriptor += stream.num_descriptors as isize;
                            i_stream += 1;
                            continue;
                        }
                        Err(err) => {
                            streams_to_remove.push((stream.id, err.into()));
                            i_descriptor += stream.num_descriptors as isize;
                            i_stream += 1;
                            continue;
                        }
                    };

                    // Get the number of available samples for reading/writing.
                    let available_samples = match get_available_samples(stream) {
                        Ok(n) => n,
                        Err(err) => {
                            streams_to_remove.push((stream.id, err.into()));
                            i_descriptor += stream.num_descriptors as isize;
                            i_stream += 1;
                            continue;
                        }
                    };

                    // Only go on if there is at least `stream.period_len` samples.
                    if available_samples < stream.period_len {
                        i_descriptor += stream.num_descriptors as isize;
                        i_stream += 1;
                        continue;
                    }

                    // Prepare the data buffer.
                    let buffer_size = stream.sample_format.sample_size() * available_samples;
                    stream.buffer.resize(buffer_size, 0u8);
                    let available_frames = available_samples / stream.num_channels as usize;

                    match stream_type {
                        StreamType::Input => {
                            let _amt = match stream.channel.io().readi(&mut stream.buffer) {
                                Err(e) => {
                                    let err =
                                        unhandled_error(format!("`snd_pcm_readi` failed: {}", e));
                                    streams_to_remove.push((stream.id, err));
                                    continue;
                                }
                                Ok(amt) => amt,
                            };

                            let input_buffer = match stream.sample_format {
                                SampleFormat::I16 => UnknownTypeInputBuffer::I16(InputBuffer {
                                    buffer: cast_input_buffer(&mut stream.buffer),
                                }),
                                SampleFormat::U16 => UnknownTypeInputBuffer::U16(InputBuffer {
                                    buffer: cast_input_buffer(&mut stream.buffer),
                                }),
                                SampleFormat::F32 => UnknownTypeInputBuffer::F32(InputBuffer {
                                    buffer: cast_input_buffer(&mut stream.buffer),
                                }),
                            };
                            let stream_data = StreamData::Input {
                                buffer: input_buffer,
                            };
                            callback(stream.id, Ok(stream_data));
                        }
                        StreamType::Output => {
                            {
                                // We're now sure that we're ready to write data.
                                let output_buffer = match stream.sample_format {
                                    SampleFormat::I16 => {
                                        UnknownTypeOutputBuffer::I16(OutputBuffer {
                                            buffer: cast_output_buffer(&mut stream.buffer),
                                        })
                                    }
                                    SampleFormat::U16 => {
                                        UnknownTypeOutputBuffer::U16(OutputBuffer {
                                            buffer: cast_output_buffer(&mut stream.buffer),
                                        })
                                    }
                                    SampleFormat::F32 => {
                                        UnknownTypeOutputBuffer::F32(OutputBuffer {
                                            buffer: cast_output_buffer(&mut stream.buffer),
                                        })
                                    }
                                };

                                let stream_data = StreamData::Output {
                                    buffer: output_buffer,
                                };
                                callback(stream.id, Ok(stream_data));
                            }
                            loop {
                                // TODO we could be more typesafe here using io_T types in alsa
                                match stream.channel.io().writei(&stream.buffer) {
                                    Err(e) => {
                                        if e.errno() == Some(Errno::EPIPE) {
                                            // buffer underrun
                                            if let Err(e) = stream.channel.try_recover(e, false) {
                                                streams_to_remove.push((
                                                    stream.id,
                                                    unhandled_error(format!(
                                                        "`snd_pcm_writei` failed: {}",
                                                        e
                                                    )),
                                                ));
                                            }
                                            continue;
                                        } else {
                                            streams_to_remove.push((
                                                stream.id,
                                                unhandled_error(format!(
                                                    "`snd_pcm_writei` failed: {}",
                                                    e
                                                )),
                                            ));
                                            continue;
                                        }
                                    }
                                    Ok(amt) => {
                                        if amt != available_frames {
                                            streams_to_remove.push((
                                                stream.id,
                                                unhandled_error(format!(
                                                    "unexpected number of frames written: expected {}, \
                                                    result {} (this should never happen)",
                                                    available_frames,
                                                    amt,
                                                ))
                                            ));
                                            continue;
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Remove any streams that have errored and notify the user.
                for (stream_id, err) in streams_to_remove {
                    run_context.streams.retain(|s| s.id != stream_id);
                    callback(stream_id, Err(err.into()));
                }
            }
        }

        panic!("`cpal::EventLoop::run` API currently disallows returning");
    }

    fn build_input_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError> {
        let pcm = PCM::new(&device.0, Direction::Capture, true).map_err(|e| {
            match e.errno() {
                // determined empirically
                Some(Errno::EBUSY) => BuildStreamError::DeviceNotAvailable,
                Some(Errno::EINVAL) => BuildStreamError::InvalidArgument,
                _ => unhandled_error(e),
            }
        })?;
        set_hw_params_from_format(&pcm, format).map_err(unhandled_error::<_, BuildStreamError>)?;
        let can_pause = pcm
            .hw_params_current()
            .map_err(unhandled_error::<_, BuildStreamError>)?
            .can_pause();
        let (buffer_len, period_len) = set_sw_params_from_format(&pcm, format)
            .map_err(unhandled_error::<_, BuildStreamError>)?;
        pcm.prepare().map_err(|e| {
            unhandled_error::<_, BuildStreamError>(format!("could not get playback handle: {}", e))
        })?;
        let num_descriptors = <PCM as PollDescriptors>::count(&pcm);
        if num_descriptors == 0 {
            return Err(unhandled_error(
                "poll descriptor count for playback stream was 0",
            ));
        }
        let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
        if new_stream_id.0 == u64::max_value() {
            return Err(BuildStreamError::StreamIdOverflow);
        }
        pcm.start().map_err(|e| {
            unhandled_error::<_, BuildStreamError>(format!("could not start capture stream: {}", e))
        })?;

        let stream_inner = StreamInner {
            id: new_stream_id.clone(),
            channel: pcm,
            sample_format: format.data_type,
            num_descriptors,
            num_channels: format.channels as u16,
            buffer_len,
            period_len,
            can_pause,
            is_paused: false,
            resume_trigger: Trigger::new(),
            buffer: vec![],
        };

        self.push_command(Command::NewStream(stream_inner));
        Ok(new_stream_id)
    }

    fn build_output_stream(
        &self,
        device: &Device,
        format: &Format,
    ) -> Result<StreamId, BuildStreamError> {
        let pcm = PCM::new(&device.0, Direction::Playback, true).map_err(|e| {
            match e.errno() {
                // determined empirically
                Some(Errno::EBUSY) => BuildStreamError::DeviceNotAvailable,
                Some(Errno::EINVAL) => BuildStreamError::InvalidArgument,
                _ => unhandled_error(e),
            }
        })?;
        set_hw_params_from_format(&pcm, format).map_err(unhandled_error::<_, BuildStreamError>)?;
        let can_pause = pcm
            .hw_params_current()
            .map_err(unhandled_error::<_, BuildStreamError>)?
            .can_pause();
        let (buffer_len, period_len) = set_sw_params_from_format(&pcm, format)
            .map_err(unhandled_error::<_, BuildStreamError>)?;
        pcm.prepare().map_err(|e| {
            unhandled_error::<_, BuildStreamError>(format!("could not get playback handle: {}", e))
        })?;
        let num_descriptors = <PCM as PollDescriptors>::count(&pcm);
        if num_descriptors == 0 {
            return Err(unhandled_error::<_, BuildStreamError>(
                "poll descriptor count for playback stream was 0",
            ));
        }
        let new_stream_id = StreamId(self.next_stream_id.fetch_add(1, Ordering::Relaxed));
        if new_stream_id.0 == u64::max_value() {
            return Err(BuildStreamError::StreamIdOverflow);
        }

        let stream_inner = StreamInner {
            id: new_stream_id.clone(),
            channel: pcm,
            sample_format: format.data_type,
            num_descriptors,
            num_channels: format.channels as u16,
            buffer_len,
            period_len,
            can_pause,
            is_paused: false,
            resume_trigger: Trigger::new(),
            buffer: vec![],
        };

        self.push_command(Command::NewStream(stream_inner));
        Ok(new_stream_id)
    }

    #[inline]
    fn push_command(&self, command: Command) {
        // Safe to unwrap: sender outlives receiver.
        self.commands.send(command).unwrap();
        self.pending_command_trigger.wakeup();
    }

    #[inline]
    fn destroy_stream(&self, stream_id: StreamId) {
        self.push_command(Command::DestroyStream(stream_id));
    }

    #[inline]
    fn play_stream(&self, stream_id: StreamId) -> Result<(), PlayStreamError> {
        self.push_command(Command::PlayStream(stream_id));
        Ok(())
    }

    #[inline]
    fn pause_stream(&self, stream_id: StreamId) -> Result<(), PauseStreamError> {
        self.push_command(Command::PauseStream(stream_id));
        Ok(())
    }
}

// Process any pending `Command`s within the `RunContext`'s queue.
fn process_commands(run_context: &mut RunContext) {
    for command in run_context.commands.try_iter() {
        match command {
            Command::DestroyStream(stream_id) => {
                run_context.streams.retain(|s| s.id != stream_id);
            }
            Command::PlayStream(stream_id) => {
                if let Some(stream) = run_context
                    .streams
                    .iter_mut()
                    .find(|stream| stream.can_pause && stream.id == stream_id)
                {
                    stream.channel.pause(false).unwrap();
                    stream.is_paused = false;
                }
            }
            Command::PauseStream(stream_id) => {
                if let Some(stream) = run_context
                    .streams
                    .iter_mut()
                    .find(|stream| stream.can_pause && stream.id == stream_id)
                {
                    stream.channel.pause(true).unwrap();
                    stream.is_paused = true;
                }
            }
            Command::NewStream(stream_inner) => {
                run_context.streams.push(stream_inner);
            }
        }
    }
}

// Resets the descriptors so that only `pending_command_trigger.read_fd()` is contained.
fn reset_descriptors_with_pending_command_trigger(
    descriptors: &mut Vec<PollFd>,
    pending_command_trigger: &Trigger,
) {
    descriptors.clear();
    descriptors.push(PollFd::new(
        pending_command_trigger.read_fd,
        PollFlags::POLLIN,
    ));
}

// Appends the `poll` descriptors for each stream onto the `RunContext`'s descriptor slice, ready
// for a call to `libc::poll`.
fn append_stream_poll_descriptors(run_context: &mut RunContext) {
    for stream in run_context.streams.iter_mut() {
        let descriptors_start = run_context.descriptors.len();
        run_context.descriptors.reserve(stream.num_descriptors);
        run_context
            .descriptors
            .extend(iter::repeat(PollFd::new(-1, PollFlags::empty())).take(stream.num_descriptors));
        // safety: repr(C) struct with single non-zero-sized field is guaranteed to have same layout as
        // inner field.
        unsafe {
            let amt = stream
                .channel
                .fill(mem::transmute(
                    &mut run_context.descriptors[descriptors_start..],
                ))
                .unwrap();
            debug_assert_eq!(stream.num_descriptors, amt);
        }
    }
}

// Poll all descriptors within the given set.
//
// Returns `Ok(true)` if some event has occurred or `Ok(false)` if no events have
// occurred.
//
// Returns an `Err` if `libc::poll` returns a negative value for some reason.
fn poll_all_descriptors(descriptors: &mut [PollFd]) -> Result<bool, BackendSpecificError> {
    // Don't timeout, wait forever.
    let number_ready = nix::poll::poll(descriptors, -1)
        .map_err(|e| unhandled_error(format!("`libc::poll()` failed: {}", e)))?;
    if number_ready == 0 {
        Ok(false)
    } else {
        Ok(true)
    }
}

// Check whether the event is `POLLOUT` or `POLLIN`.
//
// If so, return the stream type associated with the event.
//
// Otherwise, returns `Ok(None)`.
//
// Returns an `Err` if the `snd_pcm_poll_descriptors_revents` call fails.
fn check_for_pollout_or_pollin(
    stream: &StreamInner,
    stream_descriptors: &mut [PollFd],
) -> Result<Option<StreamType>, BackendSpecificError> {
    // safety: PollFd and libc::pollfd have identical memory layout.
    let revents = unsafe {
        stream
            .channel
            .revents(mem::transmute(stream_descriptors))
            .map_err(unhandled_error)?
    };
    // TODO what if both POLLOUT and POLLIN?
    Ok(if revents == POLLOUT {
        Some(StreamType::Output)
    } else if revents == POLLIN {
        Some(StreamType::Input)
    } else {
        None
    })
}

// Determine the number of samples that are available to read/write.
fn get_available_samples(stream: &StreamInner) -> Result<usize, BackendSpecificError> {
    // TODO: what about snd_pcm_avail_update?
    match stream.channel.avail() {
        Ok(v) => usize::try_from(v).map_err(|e| unhandled_error(e)),
        // TODO: Notify the user somehow.
        // buffer underrun
        Err(e) => {
            if e.errno() == Some(Errno::EPIPE) {
                Ok(stream.buffer_len)
            } else {
                Err(unhandled_error(format!(
                    "failed to get available samples: {}",
                    e
                )))
            }
        }
    }
}

fn set_hw_params_from_format(pcm: &PCM, format: &Format) -> alsa::Result<()> {
    let hw_params = HwParams::any(pcm)?;
    hw_params.set_access(Access::RWInterleaved)?;
    hw_params.set_format(match format.data_type {
        SampleFormat::I16 => AlsaFormat::s16(),
        SampleFormat::U16 => AlsaFormat::u16(),
        SampleFormat::F32 => AlsaFormat::float(),
    })?;
    hw_params.set_rate(format.sample_rate.0, ValueOr::Nearest)?;
    hw_params.set_channels(format.channels.into())?;
    // TODO: Review this. 200ms (/5) seems arbitrary...
    let buffer_size = i64::from(format.sample_rate.0 / u32::from(format.channels) / 5);
    //hw_params.set_buffer_size_max(buffer_size)?;
    println!("{:#?}", hw_params);
    pcm.hw_params(&hw_params)?;
    // testing
    let mut output = alsa::Output::buffer_open().unwrap();
    pcm.dump(&mut output);
    println!("{}", output);
    Ok(())
}

fn set_sw_params_from_format(pcm: &PCM, format: &Format) -> alsa::Result<(usize, usize)> {
    let sw_params = pcm.sw_params_current()?;
    sw_params.set_start_threshold(0)?;
    let (buffer_size, period_size) = pcm.get_params()?;
    sw_params.set_avail_min(period_size.try_into().expect("period size > i64::max"))?;
    pcm.sw_params(&sw_params)?;
    let (buffer_size, period_size) = (
        buffer_size as usize * format.channels as usize,
        period_size as usize * format.channels as usize,
    );
    Ok((buffer_size, period_size))
}

/// Cast a byte slice into a (immutable) slice of desired type.
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_input_buffer<T>(v: &[u8]) -> &[T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts(v.as_ptr() as *const T, v.len() / std::mem::size_of::<T>())
}

/// Cast a byte slice into a mutable slice of desired type.
/// Safety: it's up to the caller to ensure that the input slice has valid bit representations.
unsafe fn cast_output_buffer<T>(v: &mut [u8]) -> &mut [T] {
    debug_assert!(v.len() % std::mem::size_of::<T>() == 0);
    std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut T, v.len() / std::mem::size_of::<T>())
}

/// A simple function to help handling unexpected errors by casting them to string and wrapping
/// them in BackendSpecificError
fn unhandled_error<Inner, Outer>(error: Inner) -> Outer
where
    Inner: std::fmt::Display,
    Outer: From<BackendSpecificError>,
{
    Outer::from(BackendSpecificError {
        description: error.to_string(),
    })
}
