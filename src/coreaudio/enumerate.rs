use super::Endpoint;

use Format;

use std::vec::IntoIter as VecIntoIter;

pub struct EndpointsIterator(bool);

unsafe impl Send for EndpointsIterator {
}
unsafe impl Sync for EndpointsIterator {
}

impl Default for EndpointsIterator {
    fn default() -> Self {
        EndpointsIterator(false)
    }
}

impl Iterator for EndpointsIterator {
    type Item = Endpoint;
    fn next(&mut self) -> Option<Endpoint> {
        if self.0 {
            None
        } else {
            self.0 = true;
            Some(Endpoint)
        }
    }
}

pub fn default_endpoint() -> Option<Endpoint> {
    Some(Endpoint)
}

pub type SupportedFormatsIterator = VecIntoIter<Format>;
