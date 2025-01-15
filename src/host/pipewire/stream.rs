use std::rc::Rc;
use pipewire_client::PipewireClient;
use crate::{PauseStreamError, PlayStreamError};
use crate::traits::StreamTrait;

pub struct Stream {
    name: String,
    client: Rc<PipewireClient>,
}

impl Stream {
    pub(super) fn new(name: String, client: Rc<PipewireClient>) -> Self {
        Self {
            name,
            client,
        }
    }
}

impl StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        self.client.connect_stream(self.name.clone()).unwrap();
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        self.client.disconnect_stream(self.name.clone()).unwrap();
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.client.delete_stream(self.name.clone()).unwrap()
    }
}