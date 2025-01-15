use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ListenerTriggerPolicy {
    Keep,
    Remove
}

pub(super) struct Listener<T> {
    inner: T,
    trigger_policy: ListenerTriggerPolicy,
}

impl <T> Listener<T> {
    pub fn new(inner: T, policy: ListenerTriggerPolicy) -> Self
    {
        Self {
            inner,
            trigger_policy: policy,
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

    pub fn add(&mut self, name: String, listener: Listener<L>) {
        let mut listeners = self.listeners.borrow_mut();
        listeners.insert(name, listener);
    }

    pub fn triggered(&mut self, name: &String) {
        let mut listeners = self.listeners.borrow_mut();
        let listener = listeners.get_mut(name).unwrap();
        if listener.trigger_policy == ListenerTriggerPolicy::Remove {
            listeners.remove(name);
        }
    }
}