use crate::client::connection_string::PipewireClientInfo;
use crate::client::handlers::event::event_handler;
use crate::client::handlers::registry::registry_global_handler;
use crate::client::handlers::request::request_handler;
use crate::constants::{PIPEWIRE_CORE_SYNC_INITIALIZATION_SEQ, PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY};
use crate::error::Error;
use crate::messages::{EventMessage, MessageRequest, MessageResponse};
use crate::states::GlobalState;
use crate::utils::PipewireCoreSync;
use std::cell::RefCell;
use std::rc::Rc;

pub fn pw_thread(
    client_info: PipewireClientInfo,
    main_sender: crossbeam_channel::Sender<MessageResponse>,
    pw_receiver: pipewire::channel::Receiver<MessageRequest>,
    event_sender: pipewire::channel::Sender<EventMessage>,
    event_receiver: pipewire::channel::Receiver<EventMessage>,
) {
    let connection_properties = Some(pipewire::properties::properties! {
        PIPEWIRE_RUNTIME_DIR_ENVIRONMENT_KEY => client_info.socket_location,
        *pipewire::keys::REMOTE_NAME => client_info.socket_name,
        *pipewire::keys::APP_NAME => client_info.name,
    });

    let main_loop = match pipewire::main_loop::MainLoop::new(None) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(Error {
                    description: format!("Failed to create PipeWire main loop: {}", value),
                }))
                .unwrap();
            return;
        }
    };

    let context = match pipewire::context::Context::new(&main_loop) {
        Ok(value) => Rc::new(value),
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(Error {
                    description: format!("Failed to create PipeWire context: {}", value),
                }))
                .unwrap();
            return;
        }
    };

    let core = match context.connect(connection_properties) {
        Ok(value) => value,
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(Error {
                    description: format!("Failed to connect PipeWire server: {}", value),
                }))
                .unwrap();
            return;
        }
    };

    let listener_main_sender = main_sender.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |_, _, _, message| {
            listener_main_sender
                .send(MessageResponse::Error(Error {
                    description: format!("Server error: {}", message),
                }))
                .unwrap();
        })
        .register();

    let registry = match core.get_registry() {
        Ok(value) => Rc::new(value),
        Err(value) => {
            main_sender
                .send(MessageResponse::Error(Error {
                    description: format!("Failed to get Pipewire registry: {}", value),
                }))
                .unwrap();
            return;
        }
    };

    let core_sync = Rc::new(PipewireCoreSync::new(Rc::new(RefCell::new(core.clone()))));
    let core = Rc::new(core);
    let state = Rc::new(RefCell::new(GlobalState::default()));

    let listener_main_sender = main_sender.clone();
    core_sync.register(
        false,
        PIPEWIRE_CORE_SYNC_INITIALIZATION_SEQ,
        move || {
            listener_main_sender
                .send(MessageResponse::Initialized)
                .unwrap();
        }
    );

    let _attached_event_receiver = event_receiver.attach(
        main_loop.loop_(),
        event_handler(
            state.clone(),
            main_sender.clone(),
            event_sender.clone()
        )
    );

    let _attached_pw_receiver = pw_receiver.attach(
        main_loop.loop_(),
        request_handler(
            core.clone(),
            core_sync.clone(),
            main_loop.clone(),
            state.clone(),
            main_sender.clone()
        )
    );

    let _registry_listener = registry
        .add_listener_local()
        .global(registry_global_handler(
            state.clone(),
            registry.clone(),
            main_sender.clone(),
            event_sender.clone(),
        ))
        .global_remove(move |global_id| {
            let mut state = state.borrow_mut();
            state.remove(&global_id.into())
        })
        .register();

    main_loop.run();
}