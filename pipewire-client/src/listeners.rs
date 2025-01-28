use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub struct ListenerControlFlow {
    is_released: bool
}

impl ListenerControlFlow {
    pub fn new() -> Self {
        Self {
            is_released: false,
        }
    }
    
    pub fn is_released(&self) -> bool {
        self.is_released
    }
    
    pub fn release(&mut self) {
        if self.is_released {
            return;
        }
        self.is_released = true;
    }
}

pub(super) struct Listener<T> {
    inner: T,
    control_flow: Rc<RefCell<ListenerControlFlow>>,
}

impl <T> Listener<T> {
    pub fn new(inner: T, control_flow: Rc<RefCell<ListenerControlFlow>>) -> Self
    {
        Self {
            inner,
            control_flow,
        }
    }
}

pub(super) struct Listeners<L> {
    listeners: Rc<RefCell<HashMap<String, Listener<L>>>>,
}

impl<L> Listeners<L> {
    pub fn new() -> Self {
        Self {
            listeners: Rc::new(RefCell::new(HashMap::new())),
        }
    }
    
    pub fn get_names(&self) -> Vec<String> {
        self.listeners.borrow().keys().cloned().collect()
    }

    pub fn add(&mut self, name: String, listener: Listener<L>) {
        let mut listeners = self.listeners.borrow_mut();
        listeners.insert(name, listener);
    }

    pub fn triggered(&mut self, name: &String) {
        let mut listeners = self.listeners.borrow_mut();
        let listener = listeners.get_mut(name).unwrap();
        if listener.control_flow.borrow().is_released == false {
            return;
        }
        listeners.remove(name);
    }
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

    pub(super) fn get_listener_names(&self) -> Vec<String> {
        self.listeners.borrow().get_names()
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