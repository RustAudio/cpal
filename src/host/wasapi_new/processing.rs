use super::utils::StreamState::*;
use super::utils::*;
use crate::host::equilibrium::fill_equilibrium;
use crate::*;
use std::io;
use std::mem::MaybeUninit;
use std::time::Duration;
use tap::Pipe;
use wasapi::{AudioCaptureClient, AudioClient, AudioRenderClient, Handle};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

#[expect(clippy::too_many_arguments, reason = "this is basically just the capture context of the thread spawn closure written out")]
pub fn run_loop<Processor, DataCallback>(
    buffer_event : &Handle,
    audio_client : &AudioClient,
    processor    : &SendSync<Processor>,
    stream_state : &WaitableStreamState,
    scratch_buf  : &mut ScratchBuffer,
    data_callback: &mut DataCallback,
    fn_process   : FnProcess<Processor, DataCallback>,
    sample_format: SampleFormat,
    channel_count: usize
) -> Result<(), CpalError> {
    loop {
        wait_event(stream_state, buffer_event, None)?;

        #[deny(non_snake_case, reason = "prevent accidental wildcard patterns from typos")]
        match stream_state.get() {
            Running => fn_process(
                audio_client,
                processor,
                data_callback,
                scratch_buf,
                sample_format,
                channel_count
            )?,

            Draining if audio_client.get_current_padding()? > 0
                     => continue,

            Draining => stream_state.set(Halted, true)?,

            Halted   => continue,

            Exit     => break Ok(())
        }
    }
}

fn wait_event(
    stream_state: &WaitableStreamState,
    buffer_event: &Handle,
    timeout: Option<Duration>
) -> Result<Option<usize>, WindowsError> {
    let state_event = stream_state.event();

    if stream_state.get() == Halted {
        // The engine signals the buffer event even if the stream is halted
        // So we need to ignore it here
        return state_event
            .wait_single(timeout)?
            .then_some(0)
            .pipe(Ok);
    }

    let events = [
        buffer_event.as_handle(),
        state_event.as_raw()
    ];

    Event::wait_multiple(&events, timeout)
}

pub type FnNewProcessor<Processor> = fn(&AudioClient) -> Result<Processor, WasapiRsError>;

pub type FnProcess<Processor, DataCallback> = fn(
    audio_client : &AudioClient,
    processor    : &Processor,
    data_callback: &mut DataCallback,
    scratch_buf  : &mut ScratchBuffer,
    sample_format: SampleFormat,
    channel_count: usize
) -> Result<(), WasapiRsError>;

pub fn capture(
    _audio_client : &AudioClient, // only here to match the function signature of [`render`]
    capture_client: &AudioCaptureClient,
    data_callback : &mut (impl FnMut(&Data, &CallbackInfo) + Send + 'static),
    scratch_buf   : &mut ScratchBuffer,
    sample_format : SampleFormat,
    channel_count : usize
) -> Result<(), WasapiRsError> {
    let (frame_count, buffer_info) = capture_client.read_from_device(scratch_buf.slice())?;
    let sample_count = frame_count as usize * channel_count;

    if buffer_info.flags.silent {
        fill_equilibrium(scratch_buf.slice(), sample_format);
    } else if sample_format == SampleFormat::I24 {
        // WASAPI stores I24 in the upper bits
        scratch_buf.shift_down(sample_count);
    }

    let data = unsafe { Data::from_parts(scratch_buf.ptr.cast(), sample_count, sample_format) };
    let cbi = create_cbi(buffer_info.timestamp, buffer_info.flags.data_discontinuity);

    data_callback(&data, &cbi);

    Ok(())
}

pub fn render(
    audio_client : &AudioClient,
    render_client: &AudioRenderClient,
    data_callback: &mut (impl FnMut(&mut Data, &CallbackInfo) + Send + 'static),
    scratch_buf  : &mut ScratchBuffer,
    sample_format: SampleFormat,
    channel_count: usize
) -> Result<(), WasapiRsError> {
    let frame_count  = audio_client.get_available_space_in_frames()?;
    let sample_count = frame_count as usize * channel_count;

    let mut data = unsafe { Data::from_parts(scratch_buf.ptr.cast(), sample_count, sample_format) };
    let latency = unsafe { audio_client.as_iaudioclient().GetStreamLatency() }? as u64; // https://github.com/HEnquist/wasapi-rs/pull/83
    let cbi = create_cbi(now_hns() + latency, false);

    data_callback(&mut data, &cbi);

    if sample_format == SampleFormat::I24 {
        // WASAPI stores I24 in the upper bits
        scratch_buf.shift_up(sample_count);
    }

    render_client.write_to_device(data.bytes_mut(), None)
}

fn create_cbi(device_hns: u64, xrun: bool) -> CallbackInfo {
    CallbackInfo {
        timestamp: StreamTimestamp {
            callback: StreamInstant::from_nanos(now_hns() * 100),
            device: StreamInstant::from_nanos(device_hns * 100)
        },
        xrun
    }
}

fn now_hns() -> u64 {
    let mut count = MaybeUninit::uninit();
    let mut freq = MaybeUninit::uninit();

    let hns = unsafe {
        QueryPerformanceCounter (count.as_mut_ptr()).unwrap(); // infallible since Windows XP
        QueryPerformanceFrequency(freq.as_mut_ptr()).unwrap(); // infallible since Windows XP

        count.assume_init() as i128
            * 10_000_000
            / freq.assume_init() as i128
    };

    hns as _
}

pub fn make_spawn_error(io_error: io::Error) -> CpalError {
    Error::with_message(
        ErrorKind::ResourceExhausted,
        format!("Failed to spawn processing thread: {io_error:?}"),
    )
}