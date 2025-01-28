use std::error::Error as StdError;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct Error {
    pub description: String,
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description)
    }
}

impl StdError for Error {}