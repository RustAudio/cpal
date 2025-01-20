use crate::listeners::{Listener, ListenerControlFlow, Listeners};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Input,
    Output,
}

impl From<Direction> for pipewire::spa::utils::Direction {
    fn from(value: Direction) -> Self {
        match value {
            Direction::Input => pipewire::spa::utils::Direction::Input,
            Direction::Output => pipewire::spa::utils::Direction::Output,
        }
    }
}

pub(super) fn dict_ref_to_hashmap(dict: &pipewire::spa::utils::dict::DictRef) -> HashMap<String, String> {
    dict
        .iter()
        .map(move |(k, v)| {
            let k = String::from(k).clone();
            let v = String::from(v).clone();
            (k, v)
        })
        .collect::<HashMap<_, _>>()
}

pub(super) fn debug_dict_ref(dict: &pipewire::spa::utils::dict::DictRef) {
    for (key, value) in dict.iter() {
        println!("{} => {}", key ,value);
    }
    println!("\n");
}

pub(super) struct PipewireCoreSync {
    core: Rc<RefCell<pipewire::core::Core>>,
    listeners: Rc<RefCell<Listeners<pipewire::core::Listener>>>,
}

impl PipewireCoreSync {
    pub fn new(core: Rc<RefCell<pipewire::core::Core>>) -> Self {
        Self {
            core,
            listeners: Rc::new(RefCell::new(Listeners::new())),
        }
    }

    pub fn register<F>(&self, seq: u32, callback: F)
    where
        F: Fn(&mut ListenerControlFlow) + 'static,
    {
        let sync_id = self.core.borrow_mut().sync(seq as i32).unwrap();
        let name = format!("sync-{}", sync_id.raw());
        let listeners = self.listeners.clone();
        let listener_name = name.clone();
        let control_flow = Rc::new(RefCell::new(ListenerControlFlow::new()));
        let listener_control_flow = control_flow.clone();
        let listener = self
            .core
            .borrow_mut()
            .add_listener_local()
            .done(move |_, seq| {
                if seq != sync_id {
                    return;
                }
                if listener_control_flow.borrow().is_released() {
                    return;
                }
                callback(&mut listener_control_flow.borrow_mut());
                listeners.borrow_mut().triggered(&listener_name);
            })
            .register();
        self.listeners
            .borrow_mut()
            .add(name, Listener::new(listener, control_flow));
    }
}

impl Clone for PipewireCoreSync {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            listeners: self.listeners.clone(),
        }
    }
}

pub(super) struct Backoff {
    attempts: u32,
    maximum_attempts: u32,
    wait_duration: std::time::Duration,
    initial_wait_duration: std::time::Duration,
    maximum_wait_duration: std::time::Duration,
}

impl Backoff {
    pub fn new(
        maximum_attempts: u32,
        initial_wait_duration: std::time::Duration,
        maximum_wait_duration: std::time::Duration
    ) -> Self {
        Self {
            attempts: 0,
            maximum_attempts,
            wait_duration: initial_wait_duration,
            initial_wait_duration,
            maximum_wait_duration,
        }
    }
    
    pub fn reset(&mut self) {
        self.attempts = 0;
        self.wait_duration = self.initial_wait_duration;
    }

    pub fn retry<F, O, E>(&mut self, mut operation: F) -> Result<O, E>
    where
        F: FnMut() -> Result<O, E>,
        E: std::error::Error
    {
        self.reset();
        loop {
            let error = match operation() {
                Ok(value) => return Ok(value),
                Err(value) => value
            };
            std::thread::sleep(self.wait_duration);
            self.wait_duration = self.maximum_wait_duration.min(self.wait_duration * 2);
            self.attempts += 1;
            if self.attempts < self.maximum_attempts {
                continue;
            }
            return Err(error)
        }
    }
}