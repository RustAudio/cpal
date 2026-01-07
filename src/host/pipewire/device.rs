use std::time::Duration;
use std::{cell::RefCell, rc::Rc};

use crate::host::pipewire::stream::{StreamData, SUPPORTED_FORMATS};
use crate::{traits::DeviceTrait, DeviceDirection, SupportedStreamConfigRange};

use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};
use pipewire::{
    self as pw,
    metadata::{Metadata, MetadataListener},
    node::{Node, NodeListener},
    proxy::ProxyT,
    spa::utils::result::AsyncSeq,
};

use std::thread;

use super::stream::Stream;

pub type Devices = std::vec::IntoIter<Device>;

/// This enum record whether it is created by human or just default device
#[derive(Clone, Debug, Default, Copy)]
pub(crate) enum ClassType {
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
}

#[allow(unused)]
#[derive(Clone, Debug, Default)]
pub struct Device {
    id: u32,
    node_name: String,
    nick_name: String,
    description: String,
    direction: DeviceDirection,
    channels: u16,
    limit_quantum: u32,
    rate: u32,
    allow_rates: Vec<u32>,
    quantum: u32,
    min_quantum: u32,
    max_quantum: u32,
    class_type: ClassType,
    object_id: String,
    device_id: String,
    role: Role,
    icon_name: String,
    object_serial: u32,
}

impl Device {
    pub(crate) fn class_type(&self) -> ClassType {
        self.class_type
    }
    fn sink_default() -> Self {
        Self {
            id: 0,
            node_name: "sink_default".to_owned(),
            nick_name: "sink_default".to_owned(),
            description: "default_sink".to_owned(),
            direction: DeviceDirection::Duplex,
            channels: 2,
            class_type: ClassType::DefaultSink,
            role: Role::Sink,
            ..Default::default()
        }
    }
    fn input_default() -> Self {
        Self {
            id: 0,
            node_name: "input_default".to_owned(),
            nick_name: "input_default".to_owned(),
            description: "default_input".to_owned(),
            direction: DeviceDirection::Input,
            channels: 2,
            class_type: ClassType::DefaultInput,
            role: Role::Source,
            ..Default::default()
        }
    }
    fn output_default() -> Self {
        Self {
            id: 0,
            node_name: "output_default".to_owned(),
            nick_name: "output_default".to_owned(),
            description: "default_output".to_owned(),
            direction: DeviceDirection::Output,
            channels: 2,
            class_type: ClassType::DefaultOutput,
            role: Role::Source,
            ..Default::default()
        }
    }

    fn device_type(&self) -> crate::DeviceType {
        match self.icon_name.as_str() {
            "audio-headphones" => crate::DeviceType::Headphones,
            "audio-input-microphone" => crate::DeviceType::Microphone,
            _ => crate::DeviceType::Unknown,
        }
    }

    pub(crate) fn pw_properties(
        &self,
        direction: DeviceDirection,
    ) -> pw::properties::PropertiesBox {
        let mut properties = match direction {
            DeviceDirection::Output => pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::MEDIA_ROLE => "Music",
            },
            DeviceDirection::Input => pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Music",
            },
            _ => unreachable!(),
        };
        if matches!(self.role, Role::Sink) {
            properties.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
        }
        if matches!(self.class_type, ClassType::Node) {
            properties.insert(*pw::keys::TARGET_OBJECT, self.object_serial.to_string());
        }
        properties
    }
}
impl DeviceTrait for Device {
    type Stream = Stream;
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;

    fn id(&self) -> Result<crate::DeviceId, crate::DeviceIdError> {
        Ok(crate::DeviceId(
            crate::HostId::PipeWire,
            self.nick_name.clone(),
        ))
    }

    fn description(&self) -> Result<crate::DeviceDescription, crate::DeviceNameError> {
        Ok(crate::DeviceDescriptionBuilder::new(&self.nick_name)
            .direction(self.direction())
            .device_type(self.device_type())
            .build())
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

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, crate::SupportedStreamConfigsError> {
        if !self.supports_input() {
            return Ok(vec![].into_iter());
        }
        Ok(SUPPORTED_FORMATS
            .iter()
            .map(|sample_format| SupportedStreamConfigRange {
                channels: self.channels,
                min_sample_rate: self.rate,
                max_sample_rate: self.rate,
                buffer_size: crate::SupportedBufferSize::Range {
                    min: self.min_quantum,
                    max: self.max_quantum,
                },
                sample_format: *sample_format,
            })
            .collect::<Vec<_>>()
            .into_iter())
    }
    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, crate::SupportedStreamConfigsError> {
        if !self.supports_output() {
            return Ok(vec![].into_iter());
        }
        Ok(SUPPORTED_FORMATS
            .iter()
            .map(|sample_format| SupportedStreamConfigRange {
                channels: self.channels,
                min_sample_rate: self.rate,
                max_sample_rate: self.rate,
                buffer_size: crate::SupportedBufferSize::Range {
                    min: self.min_quantum,
                    max: self.max_quantum,
                },
                sample_format: *sample_format,
            })
            .collect::<Vec<_>>()
            .into_iter())
    }
    fn default_input_config(
        &self,
    ) -> Result<crate::SupportedStreamConfig, crate::DefaultStreamConfigError> {
        if !self.supports_input() {
            return Err(crate::DefaultStreamConfigError::StreamTypeNotSupported);
        }
        Ok(crate::SupportedStreamConfig {
            channels: self.channels,
            sample_format: crate::SampleFormat::F32,
            sample_rate: self.rate,
            buffer_size: crate::SupportedBufferSize::Range {
                min: self.min_quantum,
                max: self.max_quantum,
            },
        })
    }

    fn default_output_config(
        &self,
    ) -> Result<crate::SupportedStreamConfig, crate::DefaultStreamConfigError> {
        if !self.supports_output() {
            return Err(crate::DefaultStreamConfigError::StreamTypeNotSupported);
        }
        Ok(crate::SupportedStreamConfig {
            channels: self.channels,
            sample_format: crate::SampleFormat::F32,
            sample_rate: self.rate,
            buffer_size: crate::SupportedBufferSize::Range {
                min: self.min_quantum,
                max: self.max_quantum,
            },
        })
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &crate::StreamConfig,
        sample_format: crate::SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, crate::BuildStreamError>
    where
        D: FnMut(&crate::Data, &crate::InputCallbackInfo) + Send + 'static,
        E: FnMut(crate::StreamError) + Send + 'static,
    {
        let (pw_play_tx, pw_play_rv) = pw::channel::channel::<bool>();

        let (pw_init_tx, pw_init_rv) = std::sync::mpsc::channel::<bool>();
        let device = self.clone();
        let config = config.clone();
        let wait_timeout = timeout.unwrap_or(Duration::from_secs(2));
        let handle = thread::Builder::new()
            .name("pw_capture_music_in".to_owned())
            .spawn(move || {
                let properties = device.pw_properties(DeviceDirection::Input);
                let Ok(StreamData {
                    mainloop,
                    listener,
                    stream,
                    context,
                }) = super::stream::connect_input(
                    &config,
                    properties,
                    sample_format,
                    data_callback,
                    error_callback,
                    timeout,
                )
                else {
                    let _ = pw_init_tx.send(false);
                    return;
                };
                let _ = pw_init_tx.send(true);
                let stream = stream.clone();
                let _receiver = pw_play_rv.attach(mainloop.loop_(), move |play| {
                    let _ = stream.set_active(play);
                });
                mainloop.run();
                drop(listener);
                drop(context);
            })
            .unwrap();
        if pw_init_rv
            .recv_timeout(wait_timeout)
            .ok()
            .is_none_or(|re| !re)
        {
            return Err(crate::BuildStreamError::DeviceNotAvailable);
        };
        Ok(Stream {
            handle,
            controller: pw_play_tx,
        })
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &crate::StreamConfig,
        sample_format: crate::SampleFormat,
        data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self::Stream, crate::BuildStreamError>
    where
        D: FnMut(&mut crate::Data, &crate::OutputCallbackInfo) + Send + 'static,
        E: FnMut(crate::StreamError) + Send + 'static,
    {
        let (pw_play_tx, pw_play_rv) = pw::channel::channel::<bool>();

        let (pw_init_tx, pw_init_rv) = std::sync::mpsc::channel::<bool>();
        let device = self.clone();
        let config = config.clone();
        let wait_timeout = timeout.unwrap_or(Duration::from_secs(2));
        let handle = thread::Builder::new()
            .name("pw_capture_music_out".to_owned())
            .spawn(move || {
                let properties = device.pw_properties(DeviceDirection::Output);

                let Ok(StreamData {
                    mainloop,
                    listener,
                    stream,
                    context,
                }) = super::stream::connect_output(
                    &config,
                    properties,
                    sample_format,
                    data_callback,
                    error_callback,
                    timeout,
                )
                else {
                    let _ = pw_init_tx.send(false);
                    return;
                };

                let _ = pw_init_tx.send(true);
                let stream = stream.clone();
                let _receiver = pw_play_rv.attach(mainloop.loop_(), move |play| {
                    let _ = stream.set_active(play);
                });
                mainloop.run();
                drop(listener);
                drop(context);
            })
            .unwrap();
        if pw_init_rv
            .recv_timeout(wait_timeout)
            .ok()
            .is_none_or(|re| !re)
        {
            return Err(crate::BuildStreamError::DeviceNotAvailable);
        };
        Ok(Stream {
            handle,
            controller: pw_play_tx,
        })
    }
}

impl Device {
    pub fn channels(&self) -> u16 {
        self.channels
    }
    pub fn direction(&self) -> DeviceDirection {
        self.direction
    }
    pub fn node_name(&self) -> &str {
        &self.node_name
    }

    pub fn limit_quantam(&self) -> u32 {
        self.limit_quantum
    }
    pub fn min_quantum(&self) -> u32 {
        self.min_quantum
    }
    pub fn max_quantum(&self) -> u32 {
        self.max_quantum
    }
    pub fn quantum(&self) -> u32 {
        self.quantum
    }
    pub fn rate(&self) -> u32 {
        self.rate
    }
    pub fn allow_rates(&self) -> &[u32] {
        &self.allow_rates
    }
}

#[derive(Debug, Clone, Default)]
struct Settings {
    rate: u32,
    allow_rates: Vec<u32>,
    quantum: u32,
    min_quantum: u32,
    max_quantum: u32,
}

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

fn init_roundtrip() -> Option<Vec<Device>> {
    let mainloop = pw::main_loop::MainLoopRc::new(None).ok()?;
    let context = pw::context::ContextRc::new(&mainloop, None).ok()?;
    let core = context.connect_rc(None).ok()?;
    let registry = core.get_registry_rc().ok()?;

    // To comply with Rust's safety rules, we wrap this variable in an `Rc` and  a `Cell`.
    let devices: Rc<RefCell<Vec<Device>>> = Rc::new(RefCell::new(vec![
        Device::sink_default(),
        Device::input_default(),
        Device::output_default(),
    ]));
    let requests = Rc::new(RefCell::new(vec![]));
    let settings = Rc::new(RefCell::new(Settings::default()));
    let loop_clone = mainloop.clone();

    // Trigger the sync event. The server's answer won't be processed until we start the main loop,
    // so we can safely do this before setting up a callback. This lets us avoid using a Cell.
    let peddings: Rc<RefCell<Vec<AsyncSeq>>> = Rc::new(RefCell::new(vec![]));
    let pending = core.sync(0).expect("sync failed");

    peddings.borrow_mut().push(pending);

    let _listener_core = core
        .add_listener_local()
        .done({
            let peddings = peddings.clone();
            move |id, seq| {
                if id != pw::core::PW_ID_CORE {
                    return;
                }
                let mut peddinglist = peddings.borrow_mut();
                let Some(index) = peddinglist.iter().position(|o_seq| *o_seq == seq) else {
                    return;
                };
                peddinglist.remove(index);
                if !peddinglist.is_empty() {
                    return;
                }
                loop_clone.quit();
            }
        })
        .register();
    let _listener_reg = registry
        .add_listener_local()
        .global({
            let devices = devices.clone();
            let registry = registry.clone();
            let requests = requests.clone();
            let settings = settings.clone();
            move |global| match global.type_ {
                pipewire::types::ObjectType::Metadata => {
                    if !global.props.is_some_and(|props| {
                        props
                            .get("metadata.name")
                            .is_some_and(|name| name == "settings")
                    }) {
                        return;
                    }
                    let meta_settings: Metadata = registry.bind(global).unwrap();
                    let settings = settings.clone();
                    let listener = meta_settings
                        .add_listener_local()
                        .property(move |_, key, _, value| {
                            match (key, value) {
                                (Some("clock.rate"), Some(rate)) => {
                                    let Ok(rate) = rate.parse() else {
                                        return 0;
                                    };
                                    settings.borrow_mut().rate = rate;
                                }
                                (Some("clock.allowed-rates"), Some(list)) => {
                                    let Some(list) = list.strip_prefix("[") else {
                                        return 0;
                                    };
                                    let Some(list) = list.strip_suffix("]") else {
                                        return 0;
                                    };
                                    let list = list.trim();
                                    let list: Vec<&str> = list.split(' ').collect();
                                    let mut allow_rates = vec![];
                                    for rate in list {
                                        let Ok(rate) = rate.parse() else {
                                            return 0;
                                        };
                                        allow_rates.push(rate);
                                    }
                                    settings.borrow_mut().allow_rates = allow_rates;
                                }
                                (Some("clock.quantum"), Some(quantum)) => {
                                    let Ok(quantum) = quantum.parse() else {
                                        return 0;
                                    };
                                    settings.borrow_mut().quantum = quantum;
                                }
                                (Some("clock.min-quantum"), Some(min_quantum)) => {
                                    let Ok(min_quantum) = min_quantum.parse() else {
                                        return 0;
                                    };
                                    settings.borrow_mut().min_quantum = min_quantum;
                                }
                                (Some("clock.max-quantum"), Some(max_quantum)) => {
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
                    let pending = core.sync(0).expect("sync failed");
                    peddings.borrow_mut().push(pending);
                    requests
                        .borrow_mut()
                        .push((meta_settings.upcast(), Request::Meta(listener)));
                }
                pipewire::types::ObjectType::Node => {
                    let Some(props) = global.props else {
                        return;
                    };
                    let Some(media_class) = props.get("media.class") else {
                        return;
                    };
                    if !matches!(media_class, "Audio/Sink" | "Audio/Source") {
                        return;
                    }

                    let node: Node = registry.bind(global).expect("should ok");

                    let devices = devices.clone();
                    let listener = node
                        .add_listener_local()
                        .info(move |info| {
                            let Some(props) = info.props() else {
                                return;
                            };
                            let Some(media_class) = props.get("media.class") else {
                                return;
                            };
                            let role = match media_class {
                                "Audio/Sink" => Role::Sink,
                                "Audio/Source" => Role::Source,
                                _ => {
                                    return;
                                }
                            };
                            let Some(group) = props.get("port.group") else {
                                return;
                            };
                            let direction = match (group, role) {
                                ("playback", Role::Sink) => DeviceDirection::Duplex,
                                ("playback", Role::Source) => DeviceDirection::Input,
                                ("capture", _) => DeviceDirection::Input,
                                _ => {
                                    return;
                                }
                            };
                            let Some(object_id) = props.get("object.id") else {
                                return;
                            };
                            let Some(device_id) = props.get("device.id") else {
                                return;
                            };
                            let Some(object_serial) = props
                                .get("object.serial")
                                .and_then(|serial| serial.parse().ok())
                            else {
                                return;
                            };
                            let id = info.id();
                            let node_name = props.get("node.name").unwrap_or("unknown").to_owned();
                            let nick_name = props.get("node.nick").unwrap_or("unknown").to_owned();
                            let description = props
                                .get("node.description")
                                .unwrap_or("unknown")
                                .to_owned();
                            let channels = props
                                .get("audio.channels")
                                .and_then(|channels| channels.parse().ok())
                                .unwrap_or(2);
                            let limit_quantum: u32 = props
                                .get("clock.quantum-limit")
                                .and_then(|channels| channels.parse().ok())
                                .unwrap_or(0);
                            let icon_name = props
                                .get("device.icon_name")
                                .unwrap_or("default")
                                .to_owned();

                            let device = Device {
                                id,
                                node_name,
                                nick_name,
                                description,
                                direction,
                                role,
                                channels,
                                limit_quantum,
                                icon_name,
                                object_id: object_id.to_owned(),
                                device_id: device_id.to_owned(),
                                object_serial,
                                ..Default::default()
                            };
                            devices.borrow_mut().push(device);
                        })
                        .register();
                    let pending = core.sync(0).expect("sync failed");
                    peddings.borrow_mut().push(pending);
                    requests
                        .borrow_mut()
                        .push((node.upcast(), Request::Node(listener)));
                }
                _ => {}
            }
        })
        .register();

    mainloop.run();

    let mut devices = devices.take();
    let settings = settings.take();
    for device in devices.iter_mut() {
        device.rate = settings.rate;
        device.allow_rates = settings.allow_rates.clone();
        device.quantum = settings.quantum;
        device.min_quantum = settings.min_quantum;
        device.max_quantum = settings.max_quantum;
    }
    Some(devices)
}

pub fn init_devices() -> Option<Vec<Device>> {
    let devices = init_roundtrip()?;
    Some(devices)
}
