mod backend;
mod error;

use std::fmt::{Debug, Formatter};
pub use error::{HidError, HidResult, ErrorSource};

use crate::backend::{BackendDevice, BackendDeviceId};


#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DeviceInfo {
    id: DeviceId,
    name: String,
    product_id: u16,
    vendor_id: u16,
    usage_id: u16,
    usage_page: u16
}

impl DeviceInfo {

    pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
        backend::enumerate().await
    }

    pub async fn open(&self) -> HidResult<Device> {
        let dev = backend::open(&self.id.0).await?;
        Ok(Device {
            inner: dev,
            info: self.clone(),
        })
    }

}

pub struct Device {
    inner: BackendDevice,
    info: DeviceInfo
}


#[derive(Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct DeviceId(BackendDeviceId);

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
