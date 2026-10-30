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
///
/// The enum is `non_exhaustive`: new variants may be added in minor releases, so a `match` on it
/// needs a wildcard arm.
#[non_exhaustive]
#[derive(Debug)]
pub enum HidError {
    /// This error occurs when trying to perform an action on a device which was diconnected after being opened
    Disconnected,
    /// This error occurs when trying to open a device which is no longer connected
    NotConnected,
    /// This error occurs when the device could not be opened because it is held by another handle,
    /// for example one that was opened with [Access::Exclusive](crate::Access::Exclusive)
    /// or, on FreeBSD, any other open handle.
    /// On macOS it also occurs when a report call on an open handle is refused because another handle,
    /// in this or another process, seized the device; the handle works again once the seize is released
    Busy,
    /// This error occurs when the current backend can not provide what was requested,
    /// for example an [Access](crate::Access) mode it does not support
    Unsupported,
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
            HidError::NotConnected => f.write_str("The device is not connected"),
            HidError::Busy => f.write_str("The device is busy"),
            HidError::Unsupported => f.write_str("The operation is not supported by this backend")
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
        use windows::core::HRESULT;
        use windows::Win32::Foundation::{ERROR_DEVICE_NOT_CONNECTED, ERROR_NO_SUCH_DEVICE};

        let code = error.code();
        let disconnected = [
            HRESULT::from_win32(ERROR_DEVICE_NOT_CONNECTED.0),
            HRESULT::from_win32(ERROR_NO_SUCH_DEVICE.0)
        ];
        if disconnected.contains(&code) {
            HidError::Disconnected
        } else {
            HidError::from_backend(error)
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl From<nix::errno::Errno> for HidError {
    #[track_caller]
    fn from(error: nix::errno::Errno) -> Self {
        HidError::from_backend(nix::Error::from(error))
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::ERROR_NO_SUCH_DEVICE;

    use super::*;

    #[test]
    fn no_such_device_is_disconnected() {
        let error = windows::core::Error::from_hresult(HRESULT::from_win32(ERROR_NO_SUCH_DEVICE.0));

        assert!(matches!(HidError::from(error), HidError::Disconnected));
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
