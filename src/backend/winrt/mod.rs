mod utils;

use flume::{Receiver, TrySendError};
use futures_lite::{Stream, StreamExt};
use windows::core::{h, Ref, HSTRING};
use windows::Devices::Enumeration::{DeviceInformation};
use windows::Devices::HumanInterfaceDevice::{HidDevice, HidInputReport, HidInputReportReceivedEventArgs};
use windows::Foundation::{TypedEventHandler};
use windows::Storage::FileAccessMode;

use crate::backend::winrt::utils::{DeviceInformationSteam, IBufferExt, WinResultExt};
use crate::error::{HidResult};
use crate::{ensure, AsyncHidRead, AsyncHidWrite, Backend, DeviceInfo, HidError};

const DEVICE_SELECTOR: &HSTRING = h!(
    r#"System.Devices.InterfaceClassGuid:="{4D1E55B2-F16F-11CF-88CB-001111000030}" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True"#
);


pub struct WinRtBackend;

impl Backend for WinRtBackend {
    type DeviceId = HSTRING;
    type Reader = InputReceiver;
    type Writer = HidDevice;

    async fn enumerate() -> HidResult<impl Stream<Item=DeviceInfo<Self>> + Unpin + Send>{
        let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
            .await?;
        let devices = DeviceInformationSteam::from(devices)
            .then(|info| Box::pin(get_device_information(info)))
            .filter_map(|r| {
                r.map_err(|e| log::trace!("Failed to query device information\n\tbecause {e:?}"))
                    .ok()
                    .flatten()
            });

        Ok(devices)
    }

    async fn open(id: &Self::DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let mode = match (read, write) {
            (true, false) => FileAccessMode::Read,
            (_, true) => FileAccessMode::ReadWrite,
            (false, false) => panic!("Not supported")
        };
        let device = HidDevice::FromIdAsync(id, mode)?
            .await
            .extract_null()?
            .ok_or_else(|| HidError::message(format!("Failed to open {}", id)))?;
        let input = match read {
            true => Some(InputReceiver::new(device.clone())?),
            false => None
        };
        Ok((input, read.then_some(device)))
    }
}

async fn get_device_information(device: DeviceInformation) -> HidResult<Option<DeviceInfo<WinRtBackend>>> {
    let id = device.Id()?;
    let name = device.Name()?.to_string_lossy();
    let device = HidDevice::FromIdAsync(&id, FileAccessMode::Read)?;
    let Some(device) = device.await.extract_null()? else {
        return Ok(None);
    };
    Ok(Some(DeviceInfo {
        id,
        name,
        product_id: device.ProductId()?,
        vendor_id: device.VendorId()?,
        usage_id: device.UsageId()?,
        usage_page: device.UsagePage()?,
        // Not supported
        serial_number: None,
    }))
}

#[derive(Debug, Clone)]
pub struct InputReceiver {
    device: HidDevice,
    buffer: Receiver<HidInputReport>,
    token: i64
}

impl InputReceiver {
    fn new(device: HidDevice) -> HidResult<Self> {
        let (sender, receiver) = flume::bounded(64);
        let drain = receiver.clone();
        let token = device.InputReportReceived(&TypedEventHandler::new(move |_, args: Ref<HidInputReportReceivedEventArgs>| {
            if let Some(args) = args.as_ref() {
                let mut msg = args.Report()?;
                while let Err(TrySendError::Full(ret)) = sender.try_send(msg) {
                    log::trace!("Dropping previous input report because the queue is full");
                    let _ = drain.try_recv();
                    msg = ret;
                }
            }
            Ok(())
        }))?;
        Ok(Self { device, buffer: receiver, token })
    }

    async fn recv_async(&self) -> HidInputReport {
        self.buffer
            .recv_async()
            .await
            .expect("Input report handler got dropped unexpectedly")
    }
    
}

impl Drop for InputReceiver {
    fn drop(&mut self) {
        self.device
            .RemoveInputReportReceived(self.token)
            .unwrap_or_else(|err| log::warn!("Failed to unregister input report callback\n\t{err:?}"));
    }
}

impl AsyncHidRead for InputReceiver {
    async fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        let report = self
            .recv_async()
            .await;
        let buffer = report.Data()?;
        let buffer = buffer.as_slice()?;
        ensure!(!buffer.is_empty(), HidError::message("Input report is empty"));
        let size = buf.len().min(buffer.len());
        let start = if buffer[0] == 0x0 { 1 } else { 0 };
        buf[..(size - start)].copy_from_slice(&buffer[start..size]);

        Ok(size - start)
    }
}

impl AsyncHidWrite for HidDevice {
    async fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        let report = self.CreateOutputReport()?;

        {
            let mut buffer = report.Data()?;
            ensure!(buffer.Length()? as usize >= buf.len(), HidError::message("Output report is too large"));
            let (buffer, remainder) = buffer.as_mut_slice()?.split_at_mut(buf.len());
            buffer.copy_from_slice(buf);
            remainder.fill(0);
        }

        self.SendOutputReportAsync(&report)?.await?;
        Ok(())
    }
}


