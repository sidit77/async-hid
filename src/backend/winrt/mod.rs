use std::future::ready;
use flume::{Receiver, TrySendError};
use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use windows::core::HSTRING;
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::HumanInterfaceDevice::{HidDevice, HidInputReport, HidInputReportReceivedEventArgs};
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::h;
use windows::Storage::FileAccessMode;
use crate::DeviceInfo;
use crate::error::{ErrorSource, HidResult};

const DEVICE_SELECTOR: &HSTRING = h!(r#"System.Devices.InterfaceClassGuid:="{4D1E55B2-F16F-11CF-88CB-001111000030}" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True"#);

pub async fn enumerate() -> HidResult<Vec<DeviceInfo>> {
    let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
        .await?
        .into_iter()
        .map(get_device_information)
        .collect::<FuturesUnordered<_>>()
        .filter_map(|info| ready(info.ok()))
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
        usage_page: device.UsagePage()?,
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
        #[cfg(feature = "direct-buffer-access")]
        let size = {
            let buffer = report.Data()?;
            let buffer = to_slice(&buffer)?;
            let size = buf.len().min(buffer.len());
            buf[..size].copy_from_slice(&buffer[..size]);
            size
        };

        #[cfg(not(feature = "direct-buffer-access"))]
        let size = {
            use windows::Storage::Streams::DataReader;
            let buffer = report.Data()?;
            let size = buf.len().min(buffer.Length()? as usize);
            let reader = DataReader::FromBuffer(&buffer)?;
            reader.ReadBytes(&mut buf[..size])?;
            size
        };
        Ok(size)
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        let report = self.device.CreateOutputReport()?;
        #[cfg(feature = "direct-buffer-access")]
        {
            let mut buffer = report.Data()?;
            //TODO maybe don't panic if buf is to large
            let (buffer, remainder) = to_slice_mut(&mut buffer)?
                .split_at_mut(buf.len());
            buffer.copy_from_slice(buf);
            remainder.fill(0);
        }

        #[cfg(not(feature = "direct-buffer-access"))]
        {
            use windows::Storage::Streams::DataWriter;
            let len = report.Data()?.Length()?;
            let writer = DataWriter::new()?;
            writer.WriteBytes(&buf)?;
            for _ in 0..(len.checked_sub(buf.len() as u32).unwrap_or(0)) {
                writer.WriteByte(0)?;
            }
            report.SetData(&writer.DetachBuffer()?)?;
        }

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
        token,
    })
}

pub type BackendDeviceId = HSTRING;
pub type BackendError = windows::core::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}

#[cfg(feature = "direct-buffer-access")]
use windows::Storage::Streams::IBuffer;

#[cfg(feature = "direct-buffer-access")]
fn to_slice(buffer: &IBuffer) -> HidResult<&[u8]> {
    use windows::Win32::System::WinRT::IBufferByteAccess;
    use windows::core::ComInterface;
    let bytes: IBufferByteAccess = buffer.cast()?;
    Ok(unsafe { std::slice::from_raw_parts(bytes.Buffer()?, buffer.Length()? as usize)})
}

#[cfg(feature = "direct-buffer-access")]
fn to_slice_mut(buffer: &mut IBuffer) -> HidResult<&mut [u8]> {
    use windows::Win32::System::WinRT::IBufferByteAccess;
    use windows::core::ComInterface;
    let bytes: IBufferByteAccess = buffer.cast()?;
    Ok(unsafe { std::slice::from_raw_parts_mut(bytes.Buffer()?, buffer.Length()? as usize)})
}

