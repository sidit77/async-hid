use std::borrow::Cow;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::panic::Location;

use crate::backend::BackendError;

pub type HidResult<T> = Result<T, HidError>;

#[derive(Debug)]
pub enum ErrorSource {
    PlatformSpecific(BackendError),
    InvalidZeroSizeData,
    Custom(Cow<'static, str>)
}

pub struct HidError {
    location: &'static Location<'static>,
    source: ErrorSource
}

impl HidError {

    #[track_caller]
    pub fn custom(msg: impl Into<Cow<'static, str>>) -> Self {
        Self {
            location: Location::caller(),
            source: ErrorSource::Custom(msg.into()),
        }
    }

    #[track_caller]
    pub fn zero_sized_data() -> Self {
        Self {
            location: Location::caller(),
            source: ErrorSource::InvalidZeroSizeData,
        }
    }

}

impl Debug for HidError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "HidError: {:?}\n\tat {}", self.source, self.location)
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

#[doc(hidden)]
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $result:expr) => {
        if !($cond) {
            return Err($result);
        }
    };
}