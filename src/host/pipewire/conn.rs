extern crate pipewire;

use self::pipewire::{
    metadata::Metadata,
    node::Node,
    prelude::*,
    registry::{GlobalObject, Registry},
    spa::{Direction, ForeignDict},
    types::ObjectType,
    Core, MainLoop,
};

use std::{
    borrow::BorrowMut,
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    thread,
};

use super::device::DeviceType;

#[derive(Debug)]
enum Message {
    Terminate,
    GetSettings,
    CreateDeviceNode {
        name: String,
        device_type: DeviceType,
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
    ) -> Result<NodeInfo, String> {
        match self
            .pw_sender
            .send(Message::CreateDeviceNode { name, device_type })
        {
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
    running: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Settings {
    pub sample_rate: u32,
    pub min_buffer_size: u32,
    pub max_buffer_size: u32,
    pub default_buffer_size: u32,
}

fn pw_thread(
    main_sender: mpsc::Sender<MessageRepl>,
    pw_receiver: pipewire::channel::Receiver<Message>,
) {
    // let state = Rc::new(State::default());
    let state = Rc::new(RefCell::new(State::default()));

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
            Message::CreateDeviceNode { name, device_type } => {
                println!("Creating device");
                let node: Node = core
                    .create_object(
                        "adapter", //node_factory.get().expect("No node factory found"),
                        &pipewire::properties! {
                            *pipewire::keys::NODE_NAME => name.clone(),
                            *pipewire::keys::FACTORY_NAME => "support.null-audio-sink",
                            // *pipewire::keys::MEDIA_CLASS => match device_type {
                            //     DeviceType::InputDevice => "Audio/Sink",
                            //     DeviceType::OutputDevice => "Audio/Source"
                            // },
                            *pipewire::keys::MEDIA_CLASS => "Audio/Sink",
                            // Don't remove the object on the remote when we destroy our proxy.
                            // *pipewire::keys::OBJECT_LINGER => "1"
                        },
                    )
                    .expect("Failed to create object");

                let _list = node.add_listener_local()
                    .info(|f| {
                        println!("{:?}", f);
                    })
                    .param(|a, b, c, d| {
                        println!("{}, {}, {}, {}", a,b,c,d);
                    })
                    .register();

                do_roundtrip(&mainloop, &core, &state);
                println!("{:?}", node);

                main_sender.send(MessageRepl::NodeInfo(NodeInfo { name }));

                state.as_ref().borrow_mut().running = false;
                mainloop.quit();
            }
        }
    });

    let _listener = registry
        .add_listener_local()
        .global({
            let state = state.clone();
            let registry = registry.clone();
            let mainloop = mainloop.clone();
            let core = core.clone();

            move |global| match global.type_ {
                ObjectType::Metadata => {
                    handle_metadata(global, state.clone(), &registry, &mainloop, &core)
                }
                _ => {}
            }
        })
        .register();

    do_roundtrip(&mainloop, &core, &state);

    loop {
        if state.borrow().running {
            println!("LOOP START");
            mainloop.run();
            println!("LOOP END");
        }
    }
}

fn handle_metadata(
    metadata: &GlobalObject<ForeignDict>,
    state: Rc<RefCell<State>>,
    registry: &Rc<Registry>,
    mainloop: &MainLoop,
    core: &Rc<Core>,
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

            do_roundtrip(mainloop, core, &state);
        }
        _ => {}
    };
}

fn do_roundtrip(mainloop: &pipewire::MainLoop, core: &pipewire::Core, state: &Rc<RefCell<State>>) {
    let done = Rc::new(Cell::new(false));
    let done_clone = done.clone();
    let loop_clone = mainloop.clone();
    let state = state.clone();

    state.as_ref().borrow_mut().running = false;
    mainloop.quit();

    // Trigger the sync event. The server's answer won't be processed until we start the main loop,
    // so we can safely do this before setting up a callback. This lets us avoid using a Cell.
    let pending = core.sync(0).expect("sync failed");

    let _listener_core = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pipewire::PW_ID_CORE && seq == pending {
                done_clone.set(true);
                loop_clone.quit();
            }
        })
        .register();

    while !done.get() {
        mainloop.run();
    }

    state.as_ref().borrow_mut().running = true;
}
