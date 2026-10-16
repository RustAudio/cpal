use crate::{Error, FrameCount, traits::StreamTrait};

pub struct Stream {}

unsafe impl Send for Stream {}
unsafe impl Sync for Stream {}

impl StreamTrait for Stream {
    fn start() -> Result<(), Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn pause(&self) -> Result<(), Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn stop() -> Result<(), Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn buffer_size() -> Result<FrameCount, Error> {
        Err(Error::from(crate::ErrorKind::NotSupported))
    }

    fn now() -> StreamInstant {
        unimplemented!("No ::now() as of yet")
    }
}