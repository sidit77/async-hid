mod utils;

use flume::{Receiver, TrySendError};
use futures_lite::stream::iter;
use futures_lite::StreamExt;
use windows::core::HSTRING;
use windows::h;
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::HumanInterfaceDevice::{HidDevice, HidInputReport, HidInputReportReceivedEventArgs};
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::Storage::FileAccessMode;
use crate::backend::winrt::utils::IBufferExt;

use crate::error::{ErrorSource, HidResult};
use crate::DeviceInfo;

const DEVICE_SELECTOR: &HSTRING = h!(
    r#"System.Devices.InterfaceClassGuid:="{4D1E55B2-F16F-11CF-88CB-001111000030}" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True"#
);

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    //let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
    //    .await?
    //    .into_iter()
    //    .map(get_device_information)
    //    .collect::<FuturesUnordered<_>>()
    //    .filter_map(|info| ready(info.ok()))
    //    .collect()
    //    .await;
    let devices = iter(
        DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
        .await?
        .into_iter())
        .then(get_device_information)
        .filter_map(Result::ok)
        .collect()
        .await;
    Ok(devices)
}

async fn get_device_information(device: DeviceInformation) -> HidResult<DeviceInfo> {
    let id = device.Id()?;
    let name = device.Name()?.to_string_lossy();
    let device = HidDevice::FromIdAsync(&id, FileAccessMode::Read)?.await?;
    Ok(DeviceInfo {
        id: id.into(),
        name,
        product_id: device.ProductId()?,
        vendor_id: device.VendorId()?,
        usage_id: device.UsageId()?,
        usage_page: device.UsagePage()?
    })
}

#[derive(Debug, Clone)]
pub struct BackendDevice {
    device: HidDevice,
    input: Receiver<HidInputReport>,
    token: EventRegistrationToken
}

impl Drop for BackendDevice {
    fn drop(&mut self) {
        self.device.RemoveInputReportReceived(self.token).unwrap();
    }
}

impl BackendDevice {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        let report = self.input.recv_async().await.unwrap();
        let buffer = report.Data()?;
        let buffer = buffer.as_slice()?;
        assert!(!buffer.is_empty());
        let size = buf.len().min(buffer.len());
        let start = (buffer[0] == 0x0)
            .then_some(1)
            .unwrap_or(0);
        buf[..(size - start)].copy_from_slice(&buffer[start..size]);

        Ok(size - start)
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        let report = self.device.CreateOutputReport()?;

        let mut buffer = report.Data()?;
        //TODO maybe don't panic if buf is to large
        let (buffer, remainder) = buffer.as_mut_slice()?
            .split_at_mut(buf.len());
        buffer.copy_from_slice(buf);
        remainder.fill(0);

        self.device.SendOutputReportAsync(&report)?.await?;
        Ok(())
    }
}

pub async fn open(id: &BackendDeviceId) -> HidResult<BackendDevice> {
    let device = HidDevice::FromIdAsync(id, FileAccessMode::ReadWrite)?.await?;
    let (sender, receiver) = flume::bounded(64);
    let drain = receiver.clone();
    let token = device.InputReportReceived(&TypedEventHandler::new(move |_, args: &Option<HidInputReportReceivedEventArgs>| {
        if let Some(args) = args {
            let mut msg = args.Report()?;
            while let Err(TrySendError::Full(ret)) = sender.try_send(msg) {
                let _ = drain.try_recv();
                msg = ret;
            }
        }
        Ok(())
    }))?;
    Ok(BackendDevice {
        device,
        input: receiver,
        token
    })
}

pub type BackendDeviceId = HSTRING;
pub type BackendError = windows::core::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}
