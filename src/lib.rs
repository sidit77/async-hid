mod backend;
mod error;

pub use error::{HidError, HidResult, ErrorSource};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DeviceInfo {

}

impl DeviceInfo {

    pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
        backend::enumerate().await
    }

}

