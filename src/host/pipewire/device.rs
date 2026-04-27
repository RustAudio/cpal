use std::{
    cell::RefCell,
    rc::Rc,
    sync::{atomic::AtomicU64, Arc},
    thread,
    time::Duration,
};

use pipewire::{
    self as pw,
    metadata::{Metadata, MetadataListener},
    node::{Node, NodeListener},
    proxy::ProxyT,
    spa::utils::result::AsyncSeq,
};

use super::stream::Stream;
use crate::{
    host::pipewire::stream::{PwInitGuard, StreamCommand, StreamData, SUPPORTED_FORMATS},
    host::pipewire::utils::{audio, clock, node, DEVICE_ICON_NAME, METADATA_NAME},
    iter::{SupportedInputConfigs, SupportedOutputConfigs},
    traits::DeviceTrait,
    BufferSize, ChannelCount, Data, DeviceDescription, DeviceDescriptionBuilder, DeviceDirection,
    DeviceId, DeviceType, Error, ErrorKind, FrameCount, HostId, InputCallbackInfo, InterfaceType,
    OutputCallbackInfo, SampleFormat, SampleRate, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange,
};

pub type Devices = std::vec::IntoIter<Device>;

// This enum record whether it is created by human or just default device
#[derive(Clone, Debug, Default, Copy)]
pub(crate) enum Class {
    #[default]
    Node,
    DefaultSink,
    DefaultInput,
    DefaultOutput,
}

#[derive(Clone, Debug, Default, Copy)]
pub enum Role {
    Sink,
    #[default]
    Source,
    Duplex,
    StreamOutput,
    StreamInput,
}

#[derive(Clone, Debug, Default)]
pub struct Device {
    node_name: String,
    nick_name: String,
    description: String,
    direction: DeviceDirection,
    channels: ChannelCount,
    rate: SampleRate,
    allow_rates: Vec<SampleRate>,
    quantum: FrameCount,
    min_quantum: FrameCount,
    max_quantum: FrameCount,
    class: Class,
    role: Role,
    icon_name: String,
    object_serial: u32,
    interface_type: InterfaceType,
    address: Option<String>,
    driver: Option<String>,
}

impl Device {
    pub(crate) fn class(&self) -> Class {
        self.class
    }
    fn sink_default() -> Self {
        Self {
            node_name: "sink_default".to_owned(),
            nick_name: "sink_default".to_owned(),
            description: "default_sink".to_owned(),
            direction: DeviceDirection::Duplex,
            channels: 2,
            class: Class::DefaultSink,
            role: Role::Sink,
            ..Default::default()
        }
    }
    fn input_default() -> Self {
        Self {
            node_name: "input_default".to_owned(),
            nick_name: "input_default".to_owned(),
            description: "default_input".to_owned(),
            direction: DeviceDirection::Input,
            channels: 2,
            class: Class::DefaultInput,
            role: Role::Source,
            ..Default::default()
        }
    }
    fn output_default() -> Self {
        Self {
            node_name: "output_default".to_owned(),
            nick_name: "output_default".to_owned(),
            description: "default_output".to_owned(),
            direction: DeviceDirection::Output,
            channels: 2,
            class: Class::DefaultOutput,
            role: Role::Sink,
            ..Default::default()
        }
    }

    fn device_type(&self) -> DeviceType {
        match self.icon_name.as_str() {
            "audio-headphones" => DeviceType::Headphones,
            "audio-headset" => DeviceType::Headset,
            "audio-input-microphone" => DeviceType::Microphone,
            "audio-speakers" => DeviceType::Speaker,
            _ => DeviceType::Unknown,
        }
    }

    pub(crate) fn pw_properties(
        &self,
        direction: DeviceDirection,
        config: &StreamConfig,
    ) -> pw::properties::PropertiesBox {
        let mut properties = match direction {
            DeviceDirection::Output => pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Playback",
            },
            DeviceDirection::Input => pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Capture",
            },
            _ => unreachable!(),
        };
        if matches!(self.role, Role::Sink) && matches!(direction, DeviceDirection::Input) {
            properties.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
        }
        if matches!(self.class, Class::Node) {
            properties.insert(*pw::keys::TARGET_OBJECT, self.object_serial.to_string());
        }

        // Group input and output nodes so PipeWire schedules them in the same quantum,
        // preventing phase drift between simultaneous input/output streams.
        properties.insert("node.group", format!("cpal-{}", std::process::id()));

        if let BufferSize::Fixed(buffer_size) = config.buffer_size {
            properties.insert(*pw::keys::NODE_FORCE_QUANTUM, buffer_size.to_string());
        }
        properties
    }
}
impl DeviceTrait for Device {
    type Stream = Stream;
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;

    fn id(&self) -> Result<DeviceId, Error> {
        Ok(DeviceId(HostId::PipeWire, self.node_name.clone()))
    }

    fn description(&self) -> Result<DeviceDescription, Error> {
        let mut builder = DeviceDescriptionBuilder::new(&self.nick_name)
            .direction(self.direction)
            .device_type(self.device_type())
            .interface_type(self.interface_type);
        if let Some(address) = self.address.as_ref() {
            builder = builder.address(address);
        }
        if let Some(driver) = self.driver.as_ref() {
            builder = builder.driver(driver);
        }
        if !self.description.is_empty() && self.description != self.nick_name {
            builder = builder.add_extended_line(&self.description);
        }
        Ok(builder.build())
    }

    fn supports_input(&self) -> bool {
        matches!(
            self.direction,
            DeviceDirection::Input | DeviceDirection::Duplex
        )
    }

    fn supports_output(&self) -> bool {
        matches!(
            self.direction,
            DeviceDirection::Output | DeviceDirection::Duplex
        )
    }

    fn supported_input_configs(&self) -> Result<Self::SupportedInputConfigs, Error> {
        if !self.supports_input() {
            return Ok(vec![].into_iter());
        }
        let rates = if self.allow_rates.is_empty() {
            vec![self.rate]
        } else {
            self.allow_rates.clone()
        };
        Ok(rates
            .iter()
            .flat_map(|&rate| {
                SUPPORTED_FORMATS
                    .iter()
                    .map(move |sample_format| SupportedStreamConfigRange {
                        channels: self.channels,
                        min_sample_rate: rate,
                        max_sample_rate: rate,
                        buffer_size: SupportedBufferSize::Range {
                            min: self.min_quantum,
                            max: self.max_quantum,
                        },
                        sample_format: *sample_format,
                    })
            })
            .collect::<Vec<_>>()
            .into_iter())
    }
    fn supported_output_configs(&self) -> Result<Self::SupportedOutputConfigs, Error> {
        if !self.supports_output() {
            return Ok(vec![].into_iter());
        }
        let rates = if self.allow_rates.is_empty() {
            vec![self.rate]
        } else {
            self.allow_rates.clone()
        };
        Ok(rates
            .iter()
            .flat_map(|&rate| {
                SUPPORTED_FORMATS
                    .iter()
                    .map(move |sample_format| SupportedStreamConfigRange {
                        channels: self.channels,
                        min_sample_rate: rate,
                        max_sample_rate: rate,
                        buffer_size: SupportedBufferSize::Range {
                            min: self.min_quantum,
                            max: self.max_quantum,
                        },
                        sample_format: *sample_format,
                    })
            })
            .collect::<Vec<_>>()
            .into_iter())
    }
    fn default_input_config(&self) -> Result<SupportedStreamConfig, Error> {
        if !self.supports_input() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "device does not support input",
            ));
        }
        Ok(SupportedStreamConfig {
            channels: self.channels,
            sample_format: SampleFormat::F32,
            sample_rate: self.rate,
            buffer_size: SupportedBufferSize::Range {
                min: self.min_quantum,
                max: self.max_quantum,
            },
        })
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, Error> {
        if !self.supports_output() {
            return Err(Error::with_message(
                ErrorKind::UnsupportedOperation,
                "device does not support output",
            ));
        }
        Ok(SupportedStreamConfig {
            channels: self.channels,
            sample_format: SampleFormat::F32,
            sample_rate: self.rate,
            buffer_size: SupportedBufferSize::Range {
                min: self.min_quantum,
                max: self.max_quantum,
            },
        })
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (pw_play_tx, pw_play_rx) = pw::channel::channel::<StreamCommand>();

        let (pw_init_tx, pw_init_rx) = std::sync::mpsc::channel::<bool>();
        let device = self.clone();
        let wait_timeout = timeout.unwrap_or(Duration::from_secs(2));
        let initial_quantum = match config.buffer_size {
            BufferSize::Fixed(n) => n as u64,
            BufferSize::Default => self.quantum as u64,
        };
        let last_quantum = Arc::new(AtomicU64::new(initial_quantum));
        let last_quantum_clone = last_quantum.clone();
        let start = std::time::Instant::now();
        let handle = thread::Builder::new()
            .name("pw_in".to_owned())
            .spawn(move || {
                let _pw = PwInitGuard::new();
                let properties = device.pw_properties(DeviceDirection::Input, &config);
                let Ok(StreamData {
                    mainloop,
                    listener,
                    stream,
                    context,
                }) = super::stream::connect_input(
                    config,
                    properties,
                    sample_format,
                    data_callback,
                    error_callback,
                    last_quantum_clone,
                    start,
                )
                else {
                    let _ = pw_init_tx.send(false);
                    return;
                };
                let _ = pw_init_tx.send(true);
                let stream = stream.clone();
                let mainloop_rc1 = mainloop.clone();
                let _receiver = pw_play_rx.attach(mainloop.loop_(), move |play| match play {
                    StreamCommand::Toggle(state) => {
                        let _ = stream.set_active(state);
                    }
                    StreamCommand::Stop => {
                        let _ = stream.disconnect();
                        mainloop_rc1.quit();
                    }
                });
                mainloop.run();
                drop(listener);
                drop(context);
            })
            .map_err(|e| {
                Error::with_message(ErrorKind::Other, format!("failed to create thread: {e}"))
            })?;
        match pw_init_rx.recv_timeout(wait_timeout) {
            Ok(true) => Ok(Stream {
                handle: Some(handle),
                controller: pw_play_tx,
                last_quantum,
                start,
            }),
            Ok(false) => Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                "stream configuration rejected by PipeWire",
            )),
            Err(_) => Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                "PipeWire timed out",
            )),
        }
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, Error>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(Error) + Send + 'static,
    {
        let (pw_play_tx, pw_play_rx) = pw::channel::channel::<StreamCommand>();

        let (pw_init_tx, pw_init_rx) = std::sync::mpsc::channel::<bool>();
        let device = self.clone();
        let wait_timeout = timeout.unwrap_or(Duration::from_secs(2));
        let initial_quantum = match config.buffer_size {
            BufferSize::Fixed(n) => n as u64,
            BufferSize::Default => self.quantum as u64,
        };
        let last_quantum = Arc::new(AtomicU64::new(initial_quantum));
        let last_quantum_clone = last_quantum.clone();
        let start = std::time::Instant::now();
        let handle = thread::Builder::new()
            .name("pw_out".to_owned())
            .spawn(move || {
                let _pw = PwInitGuard::new();
                let properties = device.pw_properties(DeviceDirection::Output, &config);

                let Ok(StreamData {
                    mainloop,
                    listener,
                    stream,
                    context,
                }) = super::stream::connect_output(
                    config,
                    properties,
                    sample_format,
                    data_callback,
                    error_callback,
                    last_quantum_clone,
                    start,
                )
                else {
                    let _ = pw_init_tx.send(false);
                    return;
                };

                let _ = pw_init_tx.send(true);
                let stream = stream.clone();
                let mainloop_rc1 = mainloop.clone();
                let _receiver = pw_play_rx.attach(mainloop.loop_(), move |play| match play {
                    StreamCommand::Toggle(state) => {
                        let _ = stream.set_active(state);
                    }
                    StreamCommand::Stop => {
                        let _ = stream.disconnect();
                        mainloop_rc1.quit();
                    }
                });
                mainloop.run();
                drop(listener);
                drop(context);
            })
            .map_err(|e| {
                Error::with_message(ErrorKind::Other, format!("failed to create thread: {e}"))
            })?;
        match pw_init_rx.recv_timeout(wait_timeout) {
            Ok(true) => Ok(Stream {
                handle: Some(handle),
                controller: pw_play_tx,
                last_quantum,
                start,
            }),
            Ok(false) => Err(Error::with_message(
                ErrorKind::UnsupportedConfig,
                "stream configuration rejected by PipeWire",
            )),
            Err(_) => Err(Error::with_message(
                ErrorKind::DeviceNotAvailable,
                "PipeWire timed out",
            )),
        }
    }

    fn get_channel_name(&self, channel_index: u16, input: bool) -> Result<String, Error> {
        Err(Error::UnsupportedOperation)
    }
}

#[derive(Debug, Clone, Default)]
struct Settings {
    rate: SampleRate,
    allow_rates: Vec<SampleRate>,
    quantum: FrameCount,
    min_quantum: FrameCount,
    max_quantum: FrameCount,
}

// NOTE: it is just used to keep the lifetime
#[allow(dead_code)]
enum Request {
    Node(NodeListener),
    Meta(MetadataListener),
}

impl From<NodeListener> for Request {
    fn from(value: NodeListener) -> Self {
        Self::Node(value)
    }
}

impl From<MetadataListener> for Request {
    fn from(value: MetadataListener) -> Self {
        Self::Meta(value)
    }
}

/// Per-node rate and quantum discovered during device enumeration.
struct NodeOverrides {
    rate: Option<SampleRate>,
    quantum: Option<FrameCount>,
}

/// Parses a PipeWire fraction string like "1/48000" or "256/48000" into its parts.
fn parse_fraction(s: &str) -> Option<(u32, u32)> {
    let mut it = s.splitn(2, '/');
    let num: u32 = it.next()?.parse().ok()?;
    let den: u32 = it.next()?.parse().ok()?;
    Some((num, den))
}

fn remote_props() -> Option<pw::properties::PropertiesBox> {
    let socket = super::utils::find_socket_path()?;
    let mut props = pw::properties::PropertiesBox::new();
    props.insert(*pw::keys::REMOTE_NAME, socket.to_string_lossy().as_ref());
    Some(props)
}

pub fn init_devices() -> Option<Vec<Device>> {
    let _pw = PwInitGuard::new();
    let mainloop = pw::main_loop::MainLoopRc::new(None).ok()?;
    let context = pw::context::ContextRc::new(&mainloop, None).ok()?;
    let core = context.connect_rc(remote_props()).ok()?;
    let registry = core.get_registry_rc().ok()?;

    // Discovered hardware nodes collected during enumeration.
    let discovered: Rc<RefCell<Vec<(Device, NodeOverrides)>>> = Rc::new(RefCell::new(vec![]));
    let requests = Rc::new(RefCell::new(vec![]));
    let settings = Rc::new(RefCell::new(Settings::default()));
    let loop_clone = mainloop.clone();

    // Trigger the sync event. The server's answer won't be processed until we start the main loop,
    // so we can safely do this before setting up a callback. This lets us avoid using a Cell.
    let pending_events: Rc<RefCell<Vec<AsyncSeq>>> = Rc::new(RefCell::new(vec![]));
    let pending = core.sync(0).ok()?;

    pending_events.borrow_mut().push(pending);

    let _listener_core = core
        .add_listener_local()
        .done({
            let pending_events = pending_events.clone();
            move |id, seq| {
                if id != pw::core::PW_ID_CORE {
                    return;
                }
                let mut pendinglist = pending_events.borrow_mut();
                let Some(index) = pendinglist.iter().position(|o_seq| *o_seq == seq) else {
                    return;
                };
                pendinglist.remove(index);
                if !pendinglist.is_empty() {
                    return;
                }
                loop_clone.quit();
            }
        })
        .register();
    let _listener_reg = registry
        .add_listener_local()
        .global({
            let discovered = discovered.clone();
            let registry = registry.clone();
            let requests = requests.clone();
            let settings = settings.clone();
            move |global| match global.type_ {
                pipewire::types::ObjectType::Metadata => {
                    if !global.props.is_some_and(|props| {
                        props
                            .get(METADATA_NAME)
                            .is_some_and(|name| name == "settings")
                    }) {
                        return;
                    }
                    let meta_settings: Metadata = match registry.bind(global) {
                        Ok(meta_settings) => meta_settings,
                        Err(_) => {
                            // TODO: do something about this error
                            // Though it is already checked, but maybe something happened with
                            // pipewire?
                            return;
                        }
                    };
                    let settings = settings.clone();
                    let listener = meta_settings
                        .add_listener_local()
                        .property(move |_, key, _, value| {
                            match (key, value) {
                                (Some(clock::RATE), Some(rate)) => {
                                    let Ok(rate) = rate.parse() else {
                                        return 0;
                                    };
                                    settings.borrow_mut().rate = rate;
                                }
                                (Some(clock::ALLOWED_RATES), Some(list)) => {
                                    let Some(allow_rates) = parse_allow_rates(list) else {
                                        return 0;
                                    };

                                    settings.borrow_mut().allow_rates = allow_rates;
                                }
                                (Some(clock::QUANTUM), Some(quantum)) => {
                                    let Ok(quantum) = quantum.parse() else {
                                        return 0;
                                    };
                                    settings.borrow_mut().quantum = quantum;
                                }
                                (Some(clock::MIN_QUANTUM), Some(min_quantum)) => {
                                    let Ok(min_quantum) = min_quantum.parse() else {
                                        return 0;
                                    };
                                    settings.borrow_mut().min_quantum = min_quantum;
                                }
                                (Some(clock::MAX_QUANTUM), Some(max_quantum)) => {
                                    let Ok(max_quantum) = max_quantum.parse() else {
                                        return 0;
                                    };
                                    settings.borrow_mut().max_quantum = max_quantum;
                                }
                                _ => {}
                            }
                            0
                        })
                        .register();
                    let Ok(pending) = core.sync(0) else {
                        // TODO: maybe we should add a log?
                        return;
                    };
                    pending_events.borrow_mut().push(pending);
                    requests
                        .borrow_mut()
                        .push((meta_settings.upcast(), Request::Meta(listener)));
                }
                pipewire::types::ObjectType::Node => {
                    let Some(props) = global.props else {
                        return;
                    };
                    let Some(media_class) = props.get(*pw::keys::MEDIA_CLASS) else {
                        return;
                    };
                    if !matches!(
                        media_class,
                        audio::SINK
                            | audio::SOURCE
                            | audio::DUPLEX
                            | audio::STREAM_INPUT
                            | audio::STREAM_OUTPUT
                    ) {
                        return;
                    }

                    let node: Node = match registry.bind(global) {
                        Ok(node) => node,
                        Err(_) => {
                            // TODO: do something about this error
                            // Though it is already checked, but maybe something happened with
                            // pipewire?
                            return;
                        }
                    };

                    let discovered = discovered.clone();
                    let listener = node
                        .add_listener_local()
                        .info(move |info| {
                            let Some(props) = info.props() else {
                                return;
                            };
                            let Some(media_class) = props.get(*pw::keys::MEDIA_CLASS) else {
                                return;
                            };
                            let role = match media_class {
                                audio::SINK => Role::Sink,
                                audio::SOURCE => Role::Source,
                                audio::DUPLEX => Role::Duplex,
                                audio::STREAM_OUTPUT => Role::StreamOutput,
                                audio::STREAM_INPUT => Role::StreamInput,
                                _ => {
                                    return;
                                }
                            };
                            // Discovered `Audio/Sink` nodes are exposed as
                            // `Duplex`, so they are treated as input-capable.
                            // When cpal later opens an input stream on such
                            // a device, it sets `STREAM_CAPTURE_SINK`, which
                            // makes that stream capture audio playing to the
                            // sink.
                            let direction = match role {
                                Role::Sink => DeviceDirection::Duplex,
                                Role::Source => DeviceDirection::Input,
                                Role::Duplex => DeviceDirection::Duplex,
                                Role::StreamOutput => DeviceDirection::Output,
                                Role::StreamInput => DeviceDirection::Input,
                            };
                            let Some(object_serial) = props
                                .get(*pw::keys::OBJECT_SERIAL)
                                .and_then(|serial| serial.parse().ok())
                            else {
                                return;
                            };
                            let node_name = props
                                .get(*pw::keys::NODE_NAME)
                                .unwrap_or("unknown")
                                .to_owned();
                            let description = props
                                .get(*pw::keys::NODE_DESCRIPTION)
                                .unwrap_or("unknown")
                                .to_owned();
                            let nick_name = props
                                .get(*pw::keys::NODE_NICK)
                                .unwrap_or(description.as_str())
                                .to_owned();
                            let channels = props
                                .get(*pw::keys::AUDIO_CHANNELS)
                                .and_then(|channels| channels.parse().ok())
                                .unwrap_or(2);

                            let icon_name =
                                props.get(DEVICE_ICON_NAME).unwrap_or("default").to_owned();

                            let interface_type = match props.get(*pw::keys::DEVICE_API) {
                                Some("bluez5") => InterfaceType::Bluetooth,
                                _ => match props.get("device.bus") {
                                    Some("pci") => InterfaceType::Pci,
                                    Some("usb") => InterfaceType::Usb,
                                    Some("firewire") => InterfaceType::FireWire,
                                    Some("thunderbolt") => InterfaceType::Thunderbolt,
                                    _ => InterfaceType::Unknown,
                                },
                            };

                            let address = props
                                .get("api.bluez5.address")
                                .or_else(|| props.get("api.alsa.path"))
                                .map(|s| s.to_owned());

                            let driver = props.get(*pw::keys::FACTORY_NAME).map(|s| s.to_owned());

                            // "node.rate" = "1/<sample_rate>" — set by the driver, authoritative
                            // for the hardware clock rate.
                            let node_rate: Option<SampleRate> = props
                                .get(node::RATE)
                                .and_then(parse_fraction)
                                .filter(|(_, den)| *den > 0)
                                .map(|(_, den)| den);

                            // "node.latency" = "<frames>/<rate>" — preferred quantum; the rate
                            // denominator is a fallback when node.rate is absent.
                            let (node_quantum, latency_rate): (
                                Option<FrameCount>,
                                Option<SampleRate>,
                            ) = props
                                .get(node::LATENCY)
                                .and_then(parse_fraction)
                                .filter(|(num, den)| *num > 0 && *den > 0)
                                .unzip();

                            // node.rate is authoritative; node.latency denominator is the
                            // fallback for devices that advertise latency but not a direct rate.
                            let rate_override = node_rate.or(latency_rate);

                            let device = Device {
                                node_name,
                                nick_name,
                                description,
                                direction,
                                role,
                                channels,
                                icon_name,
                                object_serial,
                                interface_type,
                                address,
                                driver,
                                ..Default::default()
                            };
                            discovered.borrow_mut().push((
                                device,
                                NodeOverrides {
                                    rate: rate_override,
                                    quantum: node_quantum,
                                },
                            ));
                        })
                        .register();
                    let Ok(pending) = core.sync(0) else {
                        // TODO: maybe we should add a log?
                        return;
                    };
                    pending_events.borrow_mut().push(pending);
                    requests
                        .borrow_mut()
                        .push((node.upcast(), Request::Node(listener)));
                }
                _ => {}
            }
        })
        .register();

    mainloop.run();

    // If PipeWire connected but discovered no real audio nodes, it cannot route any streams. Treat
    // this as unavailable so the caller can fall back to PulseAudio or ALSA.
    if discovered.borrow().is_empty() {
        return None;
    }

    let settings = settings.take();

    // Build the three synthetic default devices and apply global clock settings to them.
    let mut devices = vec![
        Device::sink_default(),
        Device::input_default(),
        Device::output_default(),
    ];
    for device in devices.iter_mut() {
        device.rate = settings.rate;
        device.allow_rates = settings.allow_rates.clone();
        device.quantum = settings.quantum;
        device.min_quantum = settings.min_quantum;
        device.max_quantum = settings.max_quantum;
    }

    // Resolve each discovered hardware node: global settings apply unless the node
    // advertised its own rate or quantum, in which case those take precedence.
    devices.extend(
        discovered
            .take()
            .into_iter()
            .map(|(mut device, overrides)| {
                device.rate = overrides.rate.unwrap_or(settings.rate);
                device.allow_rates = settings.allow_rates.clone();
                device.quantum = overrides.quantum.unwrap_or(settings.quantum);
                device.min_quantum = settings.min_quantum;
                device.max_quantum = settings.max_quantum;
                device
            }),
    );

    Some(devices)
}

fn parse_allow_rates(list: &str) -> Option<Vec<SampleRate>> {
    let list: Vec<&str> = list
        .trim()
        .strip_prefix("[")?
        .strip_suffix("]")?
        .split(' ')
        .flat_map(|s| s.split(','))
        .filter(|s| !s.is_empty())
        .collect();
    let mut allow_rates = vec![];
    for rate in list {
        let rate = rate.parse().ok()?;
        allow_rates.push(rate);
    }
    Some(allow_rates)
}

#[cfg(test)]
mod test {
    use super::{parse_allow_rates, parse_fraction};

    #[test]
    fn rate_parse() {
        // In documents, the rates are separated by space
        let rate_str = r#"  [ 44100 48000 88200 96000 176400 192000 ] "#;
        let rates = parse_allow_rates(rate_str).unwrap();
        assert_eq!(rates, vec![44100, 48000, 88200, 96000, 176400, 192000]);
        // ',' is also allowed
        let rate_str = r#"  [ 44100, 48000, 88200, 96000 ,176400 ,192000 ] "#;
        let rates = parse_allow_rates(rate_str).unwrap();
        assert_eq!(rates, vec![44100, 48000, 88200, 96000, 176400, 192000]);
        assert_eq!(rates, vec![44100, 48000, 88200, 96000, 176400, 192000]);
        // We only use [] to define the list
        let rate_str = r#"  { 44100, 48000, 88200, 96000 ,176400 ,192000 } "#;
        let rates = parse_allow_rates(rate_str);
        assert_eq!(rates, None);
    }

    #[test]
    fn fraction_parse() {
        // node.rate format: "1/<sample_rate>"
        assert_eq!(parse_fraction("1/48000"), Some((1, 48000)));

        // node.latency format: "<quantum>/<rate>"
        assert_eq!(parse_fraction("256/48000"), Some((256, 48000)));

        // zero values are returned as-is; callers apply .filter() to reject them
        assert_eq!(parse_fraction("0/48000"), Some((0, 48000)));
        assert_eq!(parse_fraction("256/0"), Some((256, 0)));

        // invalid inputs
        assert_eq!(parse_fraction(""), None);
        assert_eq!(parse_fraction("48000"), None);
        assert_eq!(parse_fraction("abc/def"), None);
        assert_eq!(parse_fraction("/48000"), None);
        assert_eq!(parse_fraction("256/"), None);
    }
}
