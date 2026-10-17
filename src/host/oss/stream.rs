use crate::{
    traits::StreamTrait,
    StreamInstant,
    FrameCount,
    ErrorKind,
    Error,
};

use std::time::Duration;

pub struct Stream {}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl StreamTrait for Stream {
    fn start(&self) -> Result<(), Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    fn pause(&self) -> Result<(), Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    fn stop(&self, timeout: Option<Duration>) -> Result<(), Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    fn buffer_size(&self) -> Result<FrameCount, Error> {
        Err(Error::from(ErrorKind::HostUnavailable))
    }

    fn now(&self) -> StreamInstant {
        unimplemented!("No ::now() as of yet")
    }
}