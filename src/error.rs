use std::borrow::Cow;
use std::fmt::{Debug, Display, Formatter};
use std::panic::Location;

pub type HidResult<T> = Result<T, HidError>;

#[derive(Debug)]
pub enum HidError {
    Message(Cow<'static, str>),
    Other(Box<dyn std::error::Error + Send + Sync>)
}

impl HidError {
    pub fn message(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::Message(msg.into())
    }

    #[track_caller]
    pub fn from_backend(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        let error = error.into();
        log::trace!("Backend error: {} at {}", error, Location::caller());
        Self::Other(error)
    }
}

impl Display for HidError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HidError::Message(msg) => f.write_str(msg),
            HidError::Other(err) => Display::fmt(err, f)
        }
    }
}

impl std::error::Error for HidError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            HidError::Other(err) => Some(err.as_ref()),
            _ => None
        }
    }
}


#[cfg(all(target_os = "windows"))]
impl From<windows::core::Error> for HidError {
    #[track_caller]
    fn from(error: windows::core::Error) -> Self {
        HidError::from_backend(error)
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
    ($cond:expr) => {
        if !($cond) {
            return None;
        }
    };
}
