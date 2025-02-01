use crate::backend::DefaultBackend;
use crate::Backend;
use std::borrow::Cow;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::panic::Location;

pub type HidResult<T, B: Backend = DefaultBackend> = Result<T, HidError<B>>;

#[derive(Debug)]
pub enum ErrorSource<E> {
    PlatformSpecific(E),
    InvalidZeroSizeData,
    Custom(Cow<'static, str>)
}

pub struct HidError<B: Backend = DefaultBackend> {
    location: &'static Location<'static>,
    source: ErrorSource<B::Error>
}

impl<B: Backend> HidError<B> {
    #[track_caller]
    pub fn custom(msg: impl Into<Cow<'static, str>>) -> Self {
        Self {
            location: Location::caller(),
            source: ErrorSource::Custom(msg.into())
        }
    }

    #[track_caller]
    pub fn zero_sized_data() -> Self {
        Self {
            location: Location::caller(),
            source: ErrorSource::InvalidZeroSizeData
        }
    }
}

impl<B: Backend> Debug for HidError<B> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "HidError: {:?}\n\tat {}", self.source, self.location)
    }
}

impl<B: Backend> Display for HidError<B> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.source)
    }
}

impl<B: Backend> Error for HidError<B> {}

impl<B: Backend, T: Into<ErrorSource<B::Error>>> From<T> for HidError<B> {
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
