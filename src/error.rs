use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use backtrace::Backtrace;
use crate::backend::BackendError;

pub type HidResult<T> = Result<T, HidError>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ErrorSource {
    PlatformSpecific(BackendError)
}

#[derive(Clone)]
pub struct HidError {
    backtrace: Backtrace,
    source: ErrorSource
}

impl Debug for HidError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "HidError: {:?}\n{:?}", self.source, self.backtrace)
    }
}

impl Display for HidError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.source)
    }
}

impl Error for HidError { }

impl<T: Into<ErrorSource>> From<T> for HidError {
    fn from(value: T) -> Self {
        Self {
            backtrace: Backtrace::new(),
            source: value.into(),
        }
    }
}