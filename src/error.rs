use std::borrow::Cow;
use std::fmt::{Debug, Display, Formatter};
use std::panic::Location;

/// Specialized result type used for many functions in this library
pub type HidResult<T> = Result<T, HidError>;

/// The main error type of this library
/// Currently mostly a wrapper around a platform specific error
///
/// **Warning**
/// All mappings from platform specific errors to platform independent error
/// such as `Disconnected` or `NotConnected` are performed on a best effort basis,
/// as it is generally poorly documented by platform apis which operations can throw which errors and in what circumstances
#[derive(Debug)]
pub enum HidError {
    /// This error occurs when trying to perform an action on a device which was diconnected after being opened
    Disconnected,
    /// This error occurs when trying to open a device which is no longer connected
    NotConnected,
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
            HidError::Other(err) => Display::fmt(err, f),
            HidError::Disconnected => f.write_str("The device was disconnected"),
            HidError::NotConnected => f.write_str("The device is not connected")
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

impl From<std::io::Error> for HidError {
    #[track_caller]
    fn from(value: std::io::Error) -> Self {
        HidError::from_backend(value)
    }
}

#[cfg(target_os = "windows")]
impl From<windows::core::Error> for HidError {
    #[track_caller]
    fn from(error: windows::core::Error) -> Self {
        const DISCONNECTED: windows::core::HRESULT = windows::core::HRESULT::from_win32(windows::Win32::Foundation::ERROR_DEVICE_NOT_CONNECTED.0);
        match error.code() {
            DISCONNECTED => HidError::Disconnected,
            _ => HidError::from_backend(error)
        }
    }
}

#[cfg(target_os = "linux")]
impl From<nix::errno::Errno> for HidError {
    #[track_caller]
    fn from(error: nix::errno::Errno) -> Self {
        HidError::from_backend(nix::Error::from(error))
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
