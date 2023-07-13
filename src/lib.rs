mod backend;
mod error;

use std::fmt::{Debug, Formatter};
pub use error::{HidError, HidResult, ErrorSource};

use crate::backend::{BackendDevice, BackendDeviceId};


#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub product_id: u16,
    pub vendor_id: u16,
    pub usage_id: u16,
    pub usage_page: u16
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

    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }

}

pub struct Device {
    inner: BackendDevice,
    #[allow(dead_code)]
    info: DeviceInfo
}

impl Device {

    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        self.inner.read_input_report(buf).await
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        self.inner.write_output_report(buf).await
    }

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
