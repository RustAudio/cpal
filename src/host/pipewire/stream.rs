use crate::{traits::StreamTrait, PauseStreamError, PlayStreamError};

use super::Message;

#[derive(Clone)]
pub struct Stream {
    pub(super) id: usize,
    pub(super) tx: pipewire::channel::Sender<Message>,
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.tx.send(Message::DestroyStream { id: self.id });
    }
}
