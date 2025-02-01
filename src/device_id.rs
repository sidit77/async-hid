use std::fmt::{Debug, Formatter};
use crate::backend::BackendDeviceId;

/// An opaque struct that wraps the OS specific identifier of a device
#[derive(Hash, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct DeviceId(pub(crate) BackendDeviceId);

impl From<BackendDeviceId> for DeviceId {
    fn from(value: BackendDeviceId) -> Self {
        Self(value)
    }
}

impl Debug for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}