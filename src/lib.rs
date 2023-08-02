mod backend;
mod error;

use std::fmt::{Debug, Formatter};

pub use error::{ErrorSource, HidError, HidResult};

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

    pub async fn open(&self, mode: AccessMode) -> HidResult<Device> {
        let dev = backend::open(&self.id.0, mode).await?;
        Ok(Device {
            inner: dev,
            info: self.clone(),
            mode,
        })
    }

    pub fn matches(&self, usage_page: u16, usage_id: u16, vendor_id: u16, product_id: u16) -> bool {
        self.usage_page == usage_page && self.usage_id == usage_id && self.vendor_id == vendor_id && self.product_id == product_id
    }
}

pub struct Device {
    inner: BackendDevice,
    info: DeviceInfo,
    mode: AccessMode
}

impl Device {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        debug_assert!(self.mode.readable());
        self.inner.read_input_report(buf).await
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        debug_assert!(self.mode.writeable());
        self.inner.write_output_report(buf).await
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
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

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum AccessMode {
    Read,
    Write,
    #[default]
    ReadWrite
}

impl AccessMode {
    pub fn readable(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }
    pub fn writeable(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }
}