use crate::error::Error;
use std::collections::HashMap;

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

pub fn dict_ref_to_hashmap(dict: &pipewire::spa::utils::dict::DictRef) -> HashMap<String, String> {
    dict
        .iter()
        .map(move |(k, v)| {
            let k = String::from(k).clone();
            let v = String::from(v).clone();
            (k, v)
        })
        .collect::<HashMap<_, _>>()
}

pub fn debug_dict_ref(dict: &pipewire::spa::utils::dict::DictRef) {
    for (key, value) in dict.iter() {
        println!("{} => {}", key ,value);
    }
    println!("\n");
}



pub struct Backoff {
    attempts: u32,
    maximum_attempts: u32,
    wait_duration: std::time::Duration,
    initial_wait_duration: std::time::Duration,
    maximum_wait_duration: std::time::Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(
            300, // 300 attempts * 100ms = 30s
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(100)
        )
    }
}

impl Backoff {
    pub fn constant(milliseconds: u128) -> Self {
        let attempts = milliseconds / 100;
        Self::new(
            attempts as u32,
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(100)
        )
    }
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

    pub fn retry<F, O, E>(&mut self, mut operation: F) -> Result<O, Error>
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
            return Err(Error {
                description: format!("Backoff timeout: {}", error.to_string()),
            })
        }
    }
}