extern crate pipewire;

use self::pipewire::{
    metadata::Metadata,
    prelude::*,
    registry::{GlobalObject, Registry},
    spa::ForeignDict,
    types::ObjectType,
};

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    thread,
};

enum Message {
    Terminate,
    GetSettings,
}

enum MessageRepl {
    Settings(Settings),
}

pub struct PWClient {
    pw_sender: pipewire::channel::Sender<Message>,
    main_receiver: mpsc::Receiver<MessageRepl>,
}

impl PWClient {
    pub fn new() -> Self {
        let (main_sender, main_receiver) = mpsc::channel();
        let (pw_sender, pw_receiver) = pipewire::channel::channel();

        let pw_thread = thread::spawn(move || pw_thread(main_sender, pw_receiver));

        Self {
            pw_sender,
            main_receiver,
        }
    }

    pub fn get_settings(&self) -> Settings {
        self.pw_sender.send(Message::GetSettings);

        if let MessageRepl::Settings(settings) = self.main_receiver.recv().expect("Reply") {
			settings
		} else {
			Settings::default()
		}
    }
}

#[derive(Default)]
struct State {
    settings: Settings,
}

#[derive(Default, Clone)]
struct Settings {
    pub sample_rate: u32,
    pub min_buffer_size: u32,
    pub max_buffer_size: u32,
    pub default_buffer_size: u32,
}

fn pw_thread(
    main_sender: mpsc::Sender<MessageRepl>,
    pw_receiver: pipewire::channel::Receiver<Message>,
) {
    let state = Rc::new(State::default());
    // let state = Rc::new(RefCell::new(State::default()));

    let mainloop = pipewire::MainLoop::new().expect("Failed to create PipeWire Mainloop");

    let context = pipewire::Context::new(&mainloop).expect("Failed to create PipeWire Context");
    let core = context
        .connect(None)
        .expect("Failed to connect to PipeWire");
    let registry = Rc::new(core.get_registry().expect("Failed to get Registry"));

    let _receiver = pw_receiver.attach(&mainloop, |msg| {
        let mainloop = mainloop.clone();

        match msg {
            Message::Terminate => mainloop.quit(),
            Message::GetSettings => {
                main_sender.send(MessageRepl::Settings(state.settings.clone()));
            }
        }
    });

    let state_clone = state.clone();
    let _listener = registry
        .add_listener_local()
        .global(|global| match global.type_ {
            ObjectType::Metadata => handle_metadata(global, state_clone, &registry),
            _ => {}
        });

    mainloop.run();
}

fn handle_metadata(
    metadata: &GlobalObject<ForeignDict>,
    state: Rc<State>,
    registry: &Rc<Registry>,
) {
    let props = metadata
        .props
        .as_ref()
        .expect("Metadata object is missing properties");

    match props.get("metadata.name") {
        Some("settings") => {
            let settings: Metadata = registry.bind(metadata).expect("Metadata");

            settings
                .add_listener_local()
                .property(|_, key, _, value| {
                    if let Some(value) = value {
                        if let Ok(value) = value.parse::<u32>() {
                            match key {
                                Some("clock.rate") => state.settings.sample_rate = value,
                                Some("clock.quantum") => state.settings.default_buffer_size = value,
                                Some("clock.min-quantum") => state.settings.min_buffer_size = value,
                                Some("clock.max-quantum") => state.settings.max_buffer_size = value,
                                None => {}
                            };
                        }
                    }
                    0
                })
                .register();
        }
        None => {}
    };
}
