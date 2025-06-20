//! Real-time audio spectrogram visualization using terminal graphics.
//!
//! This example demonstrates:
//! - Real-time audio capture and processing
//! - Custom FFT implementation for frequency analysis
//! - Cross-platform terminal control using ANSI escape sequences
//! - Dynamic terminal resizing support
//! - Zero external UI dependencies
//!
//! ## Usage
//! ```
//! cargo run --example visualize_spectrogram
//! ```
//!
//! ## Environment Variables
//! `CPAL_WASAPI_REQUEST_FORCE_RAW=1` - On Windows, request raw (unprocessed) audio input
//! - PowerShell: `$env:CPAL_WASAPI_REQUEST_FORCE_RAW = "1"`
//! - Cmd: `set CPAL_WASAPI_REQUEST_FORCE_RAW=1`

use std::f32::consts::PI;
use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

// Global shutdown signal
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

// Configuration constants
mod config {
    use std::time::Duration;

    /// FFT window size - must be power of 2
    pub const FFT_SIZE: usize = 1024;
    
    /// Number of historical rows to display
    pub const HISTORY_ROWS: usize = 200;
    
    /// UI refresh rate
    pub const REFRESH_INTERVAL: Duration = Duration::from_millis(16);
    
    /// How often to push a new spectrogram row
    pub const ROW_UPDATE_INTERVAL: Duration = Duration::from_millis(50);
    
    /// High frequency emphasis factor
    pub const HIGH_FREQ_BOOST: f32 = 1.0;
    
    /// Minimum magnitude in dB for logarithmic scaling
    pub const MIN_DB: f32 = -60.0;
    
    /// Maximum magnitude in dB for logarithmic scaling
    pub const MAX_DB: f32 = 0.0;
}

/// Cross-platform terminal control using ANSI escape sequences
mod terminal {
    use std::io;
    #[cfg(unix)]
    use std::io::Read;

    /// Enable raw mode (platform-specific)
    pub fn enable_raw_mode() -> io::Result<()> {
        #[cfg(unix)]
        {
            unsafe {
                let mut termios: libc::termios = std::mem::zeroed();
                libc::tcgetattr(libc::STDIN_FILENO, &mut termios);
                termios.c_lflag &= !(libc::ICANON | libc::ECHO);
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios);
            }
        }
        
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            
            unsafe {
                let handle = io::stdin().as_raw_handle();
                let mut mode = 0;
                
                #[link(name = "kernel32")]
                extern "system" {
                    fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
                    fn SetConsoleMode(handle: *mut std::ffi::c_void, mode: u32) -> i32;
                }
                
                GetConsoleMode(handle as *mut _, &mut mode);
                // Disable ENABLE_LINE_INPUT and ENABLE_ECHO_INPUT
                mode &= !(0x0002 | 0x0004);
                // Enable ENABLE_VIRTUAL_TERMINAL_PROCESSING for ANSI support
                mode |= 0x0004;
                SetConsoleMode(handle as *mut _, mode);
                
                // Enable ANSI escape sequences on stdout
                let stdout_handle = io::stdout().as_raw_handle();
                GetConsoleMode(stdout_handle as *mut _, &mut mode);
                mode |= 0x0004; // ENABLE_VIRTUAL_TERMINAL_PROCESSING
                SetConsoleMode(stdout_handle as *mut _, mode);
            }
        }
        
        Ok(())
    }

    /// Disable raw mode
    pub fn disable_raw_mode() -> io::Result<()> {
        #[cfg(unix)]
        {
            unsafe {
                let mut termios: libc::termios = std::mem::zeroed();
                libc::tcgetattr(libc::STDIN_FILENO, &mut termios);
                termios.c_lflag |= libc::ICANON | libc::ECHO;
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios);
            }
        }
        
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            
            unsafe {
                let handle = io::stdin().as_raw_handle();
                
                #[link(name = "kernel32")]
                extern "system" {
                    fn SetConsoleMode(handle: *mut std::ffi::c_void, mode: u32) -> i32;
                }
                
                // Reset to a reasonable default INPUT mode.
                // Windows consoles usually start with:
                //   ENABLE_PROCESSED_INPUT (0x0001) – translates Ctrl+C / Ctrl+Break
                //   ENABLE_LINE_INPUT      (0x0002)
                //   ENABLE_ECHO_INPUT      (0x0004)
                // Previously we restored only 0x0002 | 0x0004 and *dropped*
                // PROCESSED_INPUT, which prevented the console from generating
                // control events the next time the program ran (hence Ctrl+C /
                // Ctrl+Z worked only once per terminal).  Re-enable bit 0x0001.
                const PROCESSED_INPUT: u32 = 0x0001;
                const LINE_INPUT: u32 = 0x0002;
                const ECHO_INPUT: u32 = 0x0004;
                SetConsoleMode(handle as *mut _, PROCESSED_INPUT | LINE_INPUT | ECHO_INPUT);
            }
        }
        
        Ok(())
    }

    /// Get terminal size
    pub fn size() -> io::Result<(u16, u16)> {
        #[cfg(unix)]
        {
            unsafe {
                let mut size: libc::winsize = std::mem::zeroed();
                if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) == 0 {
                    Ok((size.ws_col, size.ws_row))
                } else {
                    Ok((80, 24)) // Default fallback
                }
            }
        }
        
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            
            unsafe {
                #[repr(C)]
                struct COORD {
                    x: i16,
                    y: i16,
                }
                
                #[repr(C)]
                struct SMALL_RECT {
                    left: i16,
                    top: i16,
                    right: i16,
                    bottom: i16,
                }
                
                #[repr(C)]
                struct CONSOLE_SCREEN_BUFFER_INFO {
                    size: COORD,
                    cursor_pos: COORD,
                    attributes: u16,
                    window: SMALL_RECT,
                    max_window_size: COORD,
                }
                
                #[link(name = "kernel32")]
                extern "system" {
                    fn GetConsoleScreenBufferInfo(
                        handle: *mut std::ffi::c_void,
                        info: *mut CONSOLE_SCREEN_BUFFER_INFO
                    ) -> i32;
                }
                
                let handle = io::stdout().as_raw_handle();
                let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
                
                if GetConsoleScreenBufferInfo(handle as *mut _, &mut info) != 0 {
                    let width = info.window.right - info.window.left + 1;
                    let height = info.window.bottom - info.window.top + 1;
                    Ok((width as u16, height as u16))
                } else {
                    Ok((80, 24)) // Default fallback
                }
            }
        }
        
        #[cfg(not(any(unix, windows)))]
        {
            Ok((80, 24)) // Default for other platforms
        }
    }

    /// Check if a key was pressed (non-blocking)
    pub fn key_pressed() -> io::Result<Option<char>> {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            
            unsafe {
                let mut fds: libc::pollfd = libc::pollfd {
                    fd: io::stdin().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                
                if libc::poll(&mut fds, 1, 0) > 0 {
                    let mut buf = [0u8; 1];
                    if io::stdin().read_exact(&mut buf).is_ok() {
                        return Ok(Some(buf[0] as char));
                    }
                }
            }
        }
        
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            
            unsafe {
                #[link(name = "kernel32")]
                extern "system" {
                    fn GetNumberOfConsoleInputEvents(
                        handle: *mut std::ffi::c_void,
                        events: *mut u32
                    ) -> i32;
                    fn ReadConsoleInputA(
                        handle: *mut std::ffi::c_void,
                        buffer: *mut INPUT_RECORD,
                        length: u32,
                        read: *mut u32
                    ) -> i32;
                }
                
                #[repr(C)]
                struct INPUT_RECORD {
                    event_type: u16,
                    _padding: u16,
                    event: [u8; 16],
                }
                
                let handle = io::stdin().as_raw_handle();
                let mut event_count = 0u32;
                
                if GetNumberOfConsoleInputEvents(handle as *mut _, &mut event_count) != 0 
                    && event_count > 0 {
                    let mut buffer: INPUT_RECORD = std::mem::zeroed();
                    let mut read = 0u32;
                    
                    if ReadConsoleInputA(handle as *mut _, &mut buffer, 1, &mut read) != 0 
                        && read > 0 && buffer.event_type == 1 { // KEY_EVENT
                        // Extract ASCII char from KEY_EVENT_RECORD
                        let ascii_char = buffer.event[14];
                        if ascii_char != 0 {
                            return Ok(Some(ascii_char as char));
                        }
                    }
                }
            }
        }
        
        Ok(None)
    }

    /// ANSI escape sequences
    pub const CURSOR_HOME: &str = "\x1b[H";
    pub const HIDE_CURSOR: &str = "\x1b[?25l";
    pub const SHOW_CURSOR: &str = "\x1b[?25h";
    pub const ALTERNATE_SCREEN: &str = "\x1b[?1049h";
    pub const NORMAL_SCREEN: &str = "\x1b[?1049l";
    pub const RESET_COLOR: &str = "\x1b[0m";
    
    pub fn set_color(r: u8, g: u8, b: u8) -> String {
        format!("\x1b[38;2;{};{};{}m", r, g, b)
    }
}

/// Custom FFT implementation of a radix-2 **Cooley–Tukey** Fast Fourier Transform (FFT).
///
/// # Overview
/// This implementation is intentionally **minimal and educational** so that the
/// example remains free of additional dependencies:
/// * It accepts *real-valued* input, copies it into a complex buffer (imaginary
///   part set to `0`) and performs an **in-place** radix-2 decimation-in-time
///   FFT.
/// * The computational complexity is `O(N log N)` while the memory footprint
///   stays at `O(N)` because the supplied output buffer is re-used for the
///   transform stages.
///
/// # Possible improvements
/// Although perfectly adequate for a small real-time spectrogram, this code is
/// *not* the fastest nor the most numerically accurate solution.  If you need
/// more performance consider one (or several) of the following enhancements:
/// 1. **Drop-in replacement with `rustfft`** – The [`rustfft`](https://docs.rs/rustfft)
///    crate offers highly optimised SIMD back-ends (AVX, NEON, `simd128` for
///    WASM) and has been battle-tested in production workloads.
/// 2. **Cache twiddle factors** – The current inner loop evaluates `sin`/`cos`
///    every stage.  Pre-computing the twiddle factors for a given FFT size and
///    re-using them will remove these expensive trigonometric calls.
/// 3. **Real-to-complex (R2C) or split-radix FFT** – For purely real signals
///    only the first `N/2 + 1` bins are unique; specialised algorithms can cut
///    the work (and memory) roughly in half.
/// 4. **Iterative implementation** – Avoids recursion overhead and eliminates
///    potential recursion-depth limits on some embedded platforms.
/// 5. **Parallel or GPU execution** – Large window sizes can be divided across
///    threads or dispatched to the GPU (OpenCL, CUDA, Vulkan compute, etc.).
/// 6. **Different window functions & overlap** – Employing Hamming/Blackman
///    windows and overlapping frames (e.g. 50 % overlap) produces smoother and
///    more accurate spectrograms.
mod fft {
    use std::f32::consts::PI;
    
    #[derive(Clone, Copy, Debug)]
    pub struct Complex {
        pub re: f32,
        pub im: f32,
    }
    
    impl Complex {
        pub fn new(re: f32, im: f32) -> Self {
            Self { re, im }
        }
        
        pub fn magnitude(&self) -> f32 {
            (self.re * self.re + self.im * self.im).sqrt()
        }
        
        pub fn multiply(&self, other: &Complex) -> Complex {
            Complex {
                re: self.re * other.re - self.im * other.im,
                im: self.re * other.im + self.im * other.re,
            }
        }
    }
    
    /// Perform FFT on real-valued input
    pub fn fft_real(input: &[f32], output: &mut [Complex]) {
        let n = input.len();
        assert!(n.is_power_of_two(), "FFT size must be power of 2");
        assert_eq!(output.len(), n);
        
        // Convert real input to complex
        for (i, &sample) in input.iter().enumerate() {
            output[i] = Complex::new(sample, 0.0);
        }
        
        // Perform in-place FFT
        fft_recursive(output, false);
    }
    
    /// Recursive Cooley-Tukey FFT
    fn fft_recursive(data: &mut [Complex], inverse: bool) {
        let n = data.len();
        if n <= 1 {
            return;
        }
        
        // Bit reversal
        // Put the input sequence into *bit-reversed* order.  After this step
        // the butterfly operations that follow access contiguous memory which
        // is cache-friendly and simplifies the indexing logic.
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j ^= bit;
            
            if i < j {
                data.swap(i, j);
            }
        }
        
        // Cooley-Tukey FFT
        // After each outer loop the size of the butterfly (`len`) doubles.  The
        // `wlen` complex constant is the *principal* twiddle factor for this
        // stage; successive powers of `wlen` (managed via the accumulator `w`)
        // rotate around the unit circle to supply the correct phase shifts.
        let mut len = 2;
        while len <= n {
            let angle = 2.0 * PI / len as f32 * if inverse { 1.0 } else { -1.0 };
            let wlen = Complex::new(angle.cos(), angle.sin());
            
            let mut i = 0;
            while i < n {
                let mut w = Complex::new(1.0, 0.0);
                
                for j in 0..len / 2 {
                    let u = data[i + j];
                    let v = data[i + j + len / 2].multiply(&w);
                    
                    data[i + j] = Complex::new(u.re + v.re, u.im + v.im);
                    data[i + j + len / 2] = Complex::new(u.re - v.re, u.im - v.im);
                    
                    w = w.multiply(&wlen);
                }
                
                i += len;
            }
            
            len <<= 1;
        }
        
        // Normalize if inverse
        if inverse {
            let norm = 1.0 / n as f32;
            for c in data.iter_mut() {
                c.re *= norm;
                c.im *= norm;
            }
        }
    }
}

/// Audio capture manager
struct AudioCapture {
    stream: Stream,
    receiver: Receiver<f32>,
}

impl AudioCapture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No input device found")?;
        
        let supported_config = device.default_input_config()?;
        let config: StreamConfig = supported_config.config();
        let sample_format = supported_config.sample_format();
        
        let (sender, receiver) = mpsc::channel::<f32>();
        let stream = Self::build_stream(&device, &config, sample_format, sender)?;
        
        Ok(Self { stream, receiver })
    }
    
    fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.stream.play()?;
        Ok(())
    }
    
    fn try_recv(&self) -> Result<f32, TryRecvError> {
        self.receiver.try_recv()
    }
    
    fn build_stream(
        device: &Device,
        config: &StreamConfig,
        format: SampleFormat,
        sender: Sender<f32>,
    ) -> Result<Stream, Box<dyn std::error::Error>> {
        let error_callback = |err| eprintln!("Audio stream error: {}", err);
        
        let stream = match format {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _: &_| {
                    for &sample in data {
                        let _ = sender.send(sample);
                    }
                },
                error_callback,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], _: &_| {
                    for &sample in data {
                        let normalized = sample as f32 / i16::MAX as f32;
                        let _ = sender.send(normalized);
                    }
                },
                error_callback,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
                config,
                move |data: &[u16], _: &_| {
                    for &sample in data {
                        let centered = sample as f32 - 32768.0;
                        let normalized = centered / 32768.0;
                        let _ = sender.send(normalized);
                    }
                },
                error_callback,
                None,
            )?,
            _ => return Err(format!("Unsupported sample format: {:?}", format).into()),
        };
        
        Ok(stream)
    }
}

/// FFT analyzer
struct FftAnalyzer {
    buffer: Vec<f32>,
    output: Vec<fft::Complex>,
    position: usize,
}

impl FftAnalyzer {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            output: vec![fft::Complex::new(0.0, 0.0); size],
            position: 0,
        }
    }
    
    fn add_sample(&mut self, sample: f32) -> Option<Vec<f32>> {
        self.buffer[self.position] = sample;
        self.position += 1;
        
        if self.position >= self.buffer.len() {
            self.position = 0;
            
            // Apply Hann window to reduce spectral leakage
            let mut windowed = self.buffer.clone();
            let n = windowed.len() as f32;
            for (i, sample) in windowed.iter_mut().enumerate() {
                let window = 0.5 - 0.5 * (2.0 * PI * i as f32 / (n - 1.0)).cos();
                *sample *= window;
            }
            
            // Perform FFT
            fft::fft_real(&windowed, &mut self.output);
            
            // Return magnitudes for positive frequencies
            Some(
                self.output[..self.buffer.len() / 2]
                    .iter()
                    .map(|c| c.magnitude())
                    .collect()
            )
        } else {
            None
        }
    }
}

/// Spectrogram display
struct SpectrogramDisplay {
    history: Vec<Vec<f32>>,
    max_rows: usize,
    current_bins: usize,
    current_height: usize,
    interval_maximums: Vec<f32>,
    last_row_time: Instant,
}

impl SpectrogramDisplay {
    fn new(max_rows: usize, initial_width: usize) -> Self {
        Self {
            history: Vec::with_capacity(max_rows),
            max_rows,
            current_bins: initial_width.max(1),
            current_height: 24, // Default terminal height
            interval_maximums: vec![0.0; initial_width.max(1)],
            last_row_time: Instant::now(),
        }
    }
    
    fn update(&mut self, magnitudes: &[f32], terminal_width: usize, terminal_height: usize) -> bool {
        // Update dimensions if changed
        if terminal_width != self.current_bins && terminal_width > 0 {
            self.resize(terminal_width);
        }
        
        if terminal_height != self.current_height && terminal_height > 0 {
            self.current_height = terminal_height;
            // Adjust max_rows to fit terminal (leave space for header)
          /*   let available_rows = terminal_height.saturating_sub(3); // 3 lines for header
            if available_rows < self.max_rows {
                self.max_rows = available_rows.max(1);
                // Trim history if needed
                while self.history.len() > self.max_rows {
                    self.history.remove(0);
                }
            } */
            let available_rows = terminal_height.saturating_sub(3).max(1);
            self.max_rows = available_rows;              // grow or shrink
            if self.history.len() > self.max_rows {      // trim only when necessary
                self.history.drain(..self.history.len() - self.max_rows);
            }
        }
        
        let binned = self.bin_frequencies(magnitudes);
        let scaled = self.apply_log_scaling(&binned);
        
        for (i, &value) in scaled.iter().enumerate() {
            self.interval_maximums[i] = self.interval_maximums[i].max(value);
        }
        
        let should_update = self.last_row_time.elapsed() >= config::ROW_UPDATE_INTERVAL
            || self.history.is_empty();
        
        if should_update {
            self.add_row(self.interval_maximums.clone());
            self.interval_maximums.fill(0.0);
            self.last_row_time = Instant::now();
            true
        } else {
            false
        }
    }
    
    fn resize(&mut self, new_width: usize) {
        self.current_bins = new_width;
        self.interval_maximums = vec![0.0; new_width];
        self.history.clear();
    }
    
    fn bin_frequencies(&self, magnitudes: &[f32]) -> Vec<f32> {
        let mut binned = vec![0.0; self.current_bins];
        let step = magnitudes.len().max(1) as f32 / self.current_bins as f32;
        
        for i in 0..self.current_bins {
            let start = (i as f32 * step) as usize;
            let end = ((i + 1) as f32 * step) as usize;
            
            if start < magnitudes.len() {
                let end = end.min(magnitudes.len());
                let slice = &magnitudes[start..end];
                
                if !slice.is_empty() {
                    let avg = slice.iter().sum::<f32>() / slice.len() as f32;
                    let freq_weight = 1.0 + config::HIGH_FREQ_BOOST * (i as f32) 
                        / (self.current_bins.saturating_sub(1).max(1) as f32);
                    binned[i] = avg * freq_weight;
                }
            }
        }
        
        binned
    }
    
    fn apply_log_scaling(&self, magnitudes: &[f32]) -> Vec<f32> {
        magnitudes
            .iter()
            .map(|&mag| {
                if mag > 0.0 {
                    let db = 20.0 * mag.log10();
                    let normalized = (db - config::MIN_DB) / (config::MAX_DB - config::MIN_DB);
                    normalized.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .collect()
    }
    
    fn add_row(&mut self, row: Vec<f32>) {
        if self.history.len() >= self.max_rows {
            self.history.remove(0);
        }
        self.history.push(row);
    }
    
    fn render(&self) -> String {
        let mut output = String::new();
        
        // Move cursor to home position (no clear needed - we'll overwrite everything)
        output.push_str(terminal::CURSOR_HOME);
        
        // Title line 1
        output.push_str("Audio Spectrogram (Press CTRL+C to quit)");
        output.push_str("\x1b[0K"); // Clear to end of line
        output.push_str("\r\n");
        
        // Title line 2 - separator
        let separator_width = self.current_bins.min(self.current_height.saturating_mul(3));
        output.push_str(&"-".repeat(separator_width));
        output.push_str("\x1b[0K"); // Clear to end of line
        output.push_str("\r\n");
        
        // Calculate available rows for spectrogram
        let available_rows = self.current_height.saturating_sub(3); // 2 for header + 1 for safety
        let rows_to_render = self.history.len().min(available_rows);
        
        // Render spectrogram rows (newest at bottom)
        let start_idx = self.history.len().saturating_sub(rows_to_render);
        for row in self.history[start_idx..].iter() {
            // Ensure we don't exceed terminal width
            let cols_to_render = row.len().min(self.current_bins);
            for &value in row[..cols_to_render].iter() {
                let (r, g, b) = value_to_rgb(value);
                output.push_str(&terminal::set_color(r, g, b));
                output.push('█');
            }
            output.push_str(terminal::RESET_COLOR);
            output.push_str("\x1b[0K"); // Clear to end of line
            output.push_str("\r\n");
        }
        
        // Clear any remaining lines if terminal grew
        for _ in rows_to_render..available_rows {
            output.push_str("\x1b[0K"); // Clear entire line
            output.push_str("\r\n");
        }
        
        output
    }
}

/// Convert normalized value (0.0-1.0) to RGB color
fn value_to_rgb(value: f32) -> (u8, u8, u8) {
    let value = value.clamp(0.0, 1.0);
    
    if value < 0.5 {
        // Black to purple gradient
        let t = value * 2.0;
        let r = (127.0 * t) as u8;
        let g = 0;
        let b = (127.0 * t) as u8;
        (r, g, b)
    } else {
        // Purple to white gradient
        let t = (value - 0.5) * 2.0;
        let r = (127.0 + 128.0 * t) as u8;
        let g = (255.0 * t) as u8;
        let b = (127.0 + 128.0 * t) as u8;
        (r, g, b)
    }
}

/// Terminal UI manager
struct TerminalUI;

impl TerminalUI {
    fn setup() -> io::Result<()> {
        terminal::enable_raw_mode()?;
        print!("{}{}", terminal::ALTERNATE_SCREEN, terminal::HIDE_CURSOR);
        io::stdout().flush()?;
        Ok(())
    }
    
    fn cleanup() {
        let _ = terminal::disable_raw_mode();
        print!("{}{}{}", terminal::NORMAL_SCREEN, terminal::SHOW_CURSOR, terminal::RESET_COLOR);
        let _ = io::stdout().flush();
    }
}

impl Drop for TerminalUI {
    fn drop(&mut self) {
        Self::cleanup();
    }
}

/// Main application
struct SpectrogramApp {
    audio_capture: AudioCapture,
    fft_analyzer: FftAnalyzer,
    display: Arc<Mutex<SpectrogramDisplay>>,
    _ui: TerminalUI,
}

impl SpectrogramApp {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        TerminalUI::setup()?;
        let _ui = TerminalUI;
        
        let audio_capture = AudioCapture::new()?;
        let fft_analyzer = FftAnalyzer::new(config::FFT_SIZE);
        
        let (width, height) = terminal::size()?;
        // Adjust history rows based on terminal height
        let available_rows = height.saturating_sub(3) as usize; // Leave space for header
        let history_rows = config::HISTORY_ROWS.min(available_rows.max(5)); // At least 5 rows
        
        let display = Arc::new(Mutex::new(SpectrogramDisplay::new(
            history_rows,
            width as usize,
        )));
        
        Ok(Self {
            audio_capture,
            fft_analyzer,
            display,
            _ui,
        })
    }
    
    fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.audio_capture.start()?;
        
        let mut last_render = Instant::now();
        
        loop {
            // Check for shutdown signal
            if SHUTDOWN.load(Ordering::Relaxed) {
                break;
            }
            
            // Process audio samples
            while let Ok(sample) = self.audio_capture.try_recv() {
                if let Some(magnitudes) = self.fft_analyzer.add_sample(sample) {
                    let (width, height) = terminal::size()?;
                    
                    let mut display = self.display.lock().unwrap();
                    display.update(&magnitudes, width as usize, height as usize);
                }
            }
            
            // Render at refresh interval
            if last_render.elapsed() >= config::REFRESH_INTERVAL {
                let display = self.display.lock().unwrap();
                let output = display.render();
                drop(display); // Release lock before I/O
                
                // Write in one go for atomic update
                print!("{}", output);
                io::stdout().flush()?;
                last_render = Instant::now();
            }
            
            // Check for quit key
            if let Some(key) = terminal::key_pressed()? {
                if key == 'q' || key == '\x1b' {
                    break;
                }
            }
            
            // Small sleep to prevent CPU spinning
            thread::sleep(Duration::from_millis(1));
        }
        
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up Ctrl+C handler
    #[cfg(unix)]
    {
        unsafe {
            // Simple signal handler for SIGINT
            extern "C" fn handle_sigint(_: libc::c_int) {
                SHUTDOWN.store(true, Ordering::SeqCst);
            }
            
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handle_sigint as libc::sighandler_t;
            libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
        }
    }
    
    #[cfg(windows)]
    {
        unsafe {
            #[link(name = "kernel32")]
            extern "system" {
                fn SetConsoleCtrlHandler(
                    handler: Option<unsafe extern "system" fn(u32) -> i32>,
                    add: i32
                ) -> i32;
            }
            
            unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> i32 {
                match ctrl_type {
                    0 | 1 => { // CTRL_C_EVENT or CTRL_BREAK_EVENT
                        SHUTDOWN.store(true, Ordering::SeqCst);
                        1 // Handled
                    }
                    _ => 0, // Not handled
                }
            }
            
            SetConsoleCtrlHandler(Some(ctrl_handler), 1);
        }
    }

    // Print environment variable hint for Windows users
    #[cfg(target_os = "windows")]
    {
        if std::env::var("CPAL_WASAPI_REQUEST_FORCE_RAW").is_err() {
            eprintln!("Hint: Set CPAL_WASAPI_REQUEST_FORCE_RAW=1 to request raw audio input on Windows");
        }
    }
    
    let app = SpectrogramApp::new()?;
    app.run()?;
    
    Ok(())
} 