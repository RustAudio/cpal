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