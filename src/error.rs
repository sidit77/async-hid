use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::panic::Location;

use crate::backend::BackendError;

pub type HidResult<T> = Result<T, HidError>;

#[derive(Debug)]
pub enum ErrorSource {
    PlatformSpecific(BackendError)
}

pub struct HidError {
    location: &'static Location<'static>,
    source: ErrorSource
}

impl Debug for HidError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "HidError: {:?}\n   at {}", self.source, self.location)
    }
}

impl Display for HidError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.source)
    }
}

impl Error for HidError {}

impl<T: Into<ErrorSource>> From<T> for HidError {

    #[track_caller]
    fn from(value: T) -> Self {
        Self {
            location: Location::caller(),
            source: value.into()
        }
    }
}
