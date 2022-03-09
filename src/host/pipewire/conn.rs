extern crate pipewire;

use self::pipewire::{
    metadata::{Metadata, MetadataListener},
    node::{Node, NodeListener},
    prelude::*,
    proxy::Listener,
    registry::{GlobalObject, Registry},
    spa::{Direction, ForeignDict},
    types::ObjectType,
    Core, MainLoop,
};

use std::{
    borrow::BorrowMut,
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::mpsc,
    thread,
    time::Duration,
};

use super::device::DeviceType;

#[derive(Debug)]
enum Message {
    Terminate,
    GetSettings,
    CreateDeviceNode {
        name: String,
        device_type: DeviceType,
        autoconnect: bool,
    },
}

enum MessageRepl {
    Settings(Settings),
    NodeInfo(NodeInfo),
}

pub struct NodeInfo {
    pub name: String,
}

pub struct PWClient {
    pw_sender: pipewire::channel::Sender<Message>,
    main_receiver: mpsc::Receiver<MessageRepl>,
}

impl PWClient {
    pub fn new() -> Self {
        let (main_sender, main_receiver) = mpsc::channel();
        let (pw_sender, pw_receiver) = pipewire::channel::channel();

        let _pw_thread = thread::spawn(move || pw_thread(main_sender, pw_receiver));

        Self {
            pw_sender,
            main_receiver,
        }
    }

    pub fn get_settings(&self) -> Result<Settings, String> {
        match self.pw_sender.send(Message::GetSettings) {
            Ok(_) => match self.main_receiver.recv() {
                Ok(MessageRepl::Settings(settings)) => Ok(settings),
                Err(err) => Err(format!("{:?}", err)),
                _ => Err(format!("")),
            },
            Err(err) => Err(format!("{:?}", err)),
        }
    }

    pub fn create_device_node(
        &self,
        name: String,
        device_type: DeviceType,
        connect_ports_automatically: bool,
    ) -> Result<NodeInfo, String> {
        match self.pw_sender.send(Message::CreateDeviceNode {
            name,
            device_type,
            autoconnect: connect_ports_automatically,
        }) {
            Ok(_) => match self.main_receiver.recv() {
                Ok(MessageRepl::NodeInfo(info)) => Ok(info),
                Err(err) => Err(format!("{:?}", err)),
                _ => Err(format!("")),
            },
            Err(err) => Err(format!("{:?}", err)),
        }
    }
}

#[derive(Default)]
struct State {
    settings: Settings,
    nodes: Vec<Node>,
}

#[derive(Default, Clone, Debug)]
pub struct Settings {
    pub sample_rate: u32,
    pub min_buffer_size: u32,
    pub max_buffer_size: u32,
    pub default_buffer_size: u32,
}

enum ProxyItem {
    Metadata {
        _proxy: Metadata,
        _listener: MetadataListener,
    },
    Node {
        _proxy: Node,
        _listener: NodeListener,
    },
}

fn pw_thread(
    main_sender: mpsc::Sender<MessageRepl>,
    pw_receiver: pipewire::channel::Receiver<Message>,
) {
    pipewire::init();
    // let state = Rc::new(State::default());
    let state = Rc::new(RefCell::new(State::default()));
    let proxies = Rc::new(RefCell::new(HashMap::<u32, ProxyItem>::new()));

    let mainloop = pipewire::MainLoop::new().expect("Failed to create PipeWire Mainloop");

    let context = pipewire::Context::new(&mainloop).expect("Failed to create PipeWire Context");
    let core = Rc::new(
        context
            .connect(None)
            .expect("Failed to connect to PipeWire"),
    );
    let registry = Rc::new(core.get_registry().expect("Failed to get Registry"));

    let _receiver = pw_receiver.attach(&mainloop, {
        let mainloop = mainloop.clone();
        let state = state.clone();
        let main_sender = main_sender.clone();
        let core = core.clone();

        move |msg| match msg {
            Message::Terminate => mainloop.quit(),
            Message::GetSettings => {
                let settings = state.borrow().settings.clone();
                main_sender.send(MessageRepl::Settings(settings));
            }
            Message::CreateDeviceNode {
                name,
                device_type,
                autoconnect,
            } => {
                let node: Node = core
                    .create_object(
                        "adapter", //node_factory.get().expect("No node factory found"),
                        &pipewire::properties! {
                            *pipewire::keys::NODE_NAME => name.clone(),
                            *pipewire::keys::FACTORY_NAME => "support.null-audio-sink",
                            *pipewire::keys::MEDIA_TYPE => "Audio",
                            *pipewire::keys::MEDIA_CATEGORY => match device_type {
                                    DeviceType::InputDevice => "Capture",
                                    DeviceType::OutputDevice => "Playback"
                            },
                            *pipewire::keys::NODE_AUTOCONNECT => match autoconnect {
                                false => "false",
                                true => "true",
                            },
                            // Don't remove the object on the remote when we destroy our proxy.
                            // *pipewire::keys::OBJECT_LINGER => "1"
                        },
                    )
                    .expect("Failed to create object");

                let _listener = node
                    .add_listener_local()
                    .info(|f| {
                        println!("{:?}", f);
                    })
                    .param(|a, b, c, d| {
                        println!("{}, {}, {}, {}", a, b, c, d);
                    })
                    .register();

                println!("{:?}", node);

                state.as_ref().borrow_mut().nodes.push(node);

                // proxies.as_ref().borrow_mut().insert(
                //     node.proxy.id(),
                //     ProxyItem::Node {
                //         _proxy: node,
                //         _listener,
                //     },
                // );

                main_sender.send(MessageRepl::NodeInfo(NodeInfo { name }));
            }
        }
    });

    let _reg_listener = registry
        .add_listener_local()
        .global({
            let state = state.clone();
            let registry = registry.clone();
            let proxies = proxies.clone();

            move |global| match global.type_ {
                ObjectType::Metadata => handle_metadata(global, &state, &registry, &proxies),
                _ => {}
            }
        })
        .register();

    // let timer = mainloop.add_timer({
    //     move |_| {
    //     }
    // });

    // timer
    //     .update_timer(
    //         Some(Duration::from_millis(500)),
    //         Some(Duration::from_secs(1)),
    //     )
    //     .into_result()
    //     .expect("FU");

    mainloop.run();
}

fn handle_metadata(
    metadata: &GlobalObject<ForeignDict>,
    state: &Rc<RefCell<State>>,
    registry: &Rc<Registry>,
    proxies: &Rc<RefCell<HashMap<u32, ProxyItem>>>,
) {
    let props = metadata
        .props
        .as_ref()
        .expect("Metadata object is missing properties");

    match props.get("metadata.name") {
        Some("settings") => {
            let settings: Metadata = registry.bind(metadata).expect("Metadata");

            let _listener = settings
                .add_listener_local()
                .property({
                    let state = state.clone();
                    move |_, key, _, value| {
                        let mut state = state.as_ref().borrow_mut();
                        if let Some(value) = value {
                            if let Ok(value) = value.parse::<u32>() {
                                match key {
                                    Some("clock.rate") => state.settings.sample_rate = value,
                                    Some("clock.quantum") => {
                                        state.settings.default_buffer_size = value
                                    }
                                    Some("clock.min-quantum") => {
                                        state.settings.min_buffer_size = value
                                    }
                                    Some("clock.max-quantum") => {
                                        state.settings.max_buffer_size = value
                                    }
                                    _ => {}
                                };
                            }
                        }
                        0
                    }
                })
                .register();

            proxies.as_ref().borrow_mut().insert(
                metadata.id,
                ProxyItem::Metadata {
                    _proxy: settings,
                    _listener,
                },
            );
        }
        _ => {}
    };
}
