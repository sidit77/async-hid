use crate::{AccessMode, DeviceInfo, ErrorSource, HidResult};

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    Ok(Vec::new())
}


#[derive(Debug, Clone)]
pub struct BackendDevice;


pub async fn open(_id: &BackendDeviceId, _mode: AccessMode) -> HidResult<BackendDevice> {
    unimplemented!()
}

impl BackendDevice {
    pub async fn read_input_report(&self, _buf: &mut [u8]) -> HidResult<usize> {
        unimplemented!()
    }

    pub async fn write_output_report(&self, _buf: &[u8]) -> HidResult<()> {
        unimplemented!()
    }
}

pub type BackendDeviceId = String;
pub type BackendError = ();

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}