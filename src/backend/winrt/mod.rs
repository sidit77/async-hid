mod utils;

use std::sync::{Arc, Mutex};
use async_channel::{Receiver, SendError, Sender};
use futures_lite::{StreamExt};
use log::{debug};
use once_cell::sync::OnceCell;
use windows::core::{h, Ref, HRESULT, HSTRING};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher, DeviceWatcherStatus};
use windows::Devices::HumanInterfaceDevice::{HidDevice, HidInputReport, HidInputReportReceivedEventArgs};
use windows::Foundation::{TypedEventHandler};
use windows::Storage::FileAccessMode;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use crate::backend::winrt::utils::{DeviceInformationSteam, IBufferExt, WinResultExt};
use crate::error::{HidResult};
use crate::{ensure, AsyncHidRead, AsyncHidWrite, DeviceInfo, HidError};
use crate::backend::{Backend, DeviceInfoStream};
use crate::device_info::DeviceId;

const DEVICE_SELECTOR: &HSTRING = h!(
    r#"System.Devices.InterfaceClassGuid:="{4D1E55B2-F16F-11CF-88CB-001111000030}" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True"#
);

struct DeviceWatcherContext {
    watcher: DeviceWatcher,
    mutex: Mutex<Vec<(HSTRING, Sender<HidInputReport>)>>
}

#[derive(Default, Clone)]
pub struct WinRtBackend {
    inner: Arc<OnceCell<DeviceWatcherContext>>
}

impl WinRtBackend {
    
    fn get_watcher_context(&self) -> HidResult<&DeviceWatcherContext> {
        let ctx = self.inner.get_or_try_init(|| {
            let watcher = DeviceInformation::CreateWatcherAqsFilter(DEVICE_SELECTOR)?;
            
            watcher.Removed(&TypedEventHandler::new({
                let ctx = Arc::downgrade(&self.inner);
                move |_, info: Ref<DeviceInformationUpdate>| {
                    let info = info.unwrap();
                    let id = info.Id()?;
                    debug!("Removed: {:?}", id);
                    if let Some(ctx) = ctx.upgrade() {
                        let inner = ctx.get().expect("Inner should be initialized");
                        inner.mutex.lock().unwrap().retain(|(rid, channel)| match rid == &id {
                            true => {
                                debug!("Force closing channel");
                                channel.close();
                                false
                            }
                            false => true
                        });
                    }
                    
                    Ok(())
                }
            }))?;
            Ok::<_, HidError>(DeviceWatcherContext {
                watcher,
                mutex: Mutex::new(Default::default()),
            })
        })?;
        
        if ctx.watcher.Status()? == DeviceWatcherStatus::Created {
            debug!("starting device watcher");
            ctx.watcher.Start()?;
        }
        
        Ok(ctx)
    }
    
}

impl Backend for WinRtBackend {
    // type DeviceId = HSTRING;
    type Reader = InputReceiver;
    type Writer = HidDevice;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream>{
        let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
            .await?;
        let devices = DeviceInformationSteam::from(devices)
            .then(|info| Box::pin(get_device_information(info)))
            .filter_map(|r| r.transpose());

        Ok(devices.boxed())
    }

    async fn open(&self, id: &DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let mode = match (read, write) {
            (true, false) => FileAccessMode::Read,
            (_, true) => FileAccessMode::ReadWrite,
            (false, false) => panic!("Not supported")
        };
        let DeviceId::UncPath(id) = id;
        let device = HidDevice::FromIdAsync(id, mode)?
            .await
            .map_err(|err| match err {
                e if e.code().is_ok() || e.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => HidError::NotConnected,
                e => e.into()
            })?;
        let input = match read {
            true => Some(InputReceiver::new(self, id, device.clone())?),
            false => None
        };
        Ok((input, read.then_some(device)))
    }
}

async fn get_device_information(device: DeviceInformation) -> HidResult<Option<DeviceInfo>> {
    let id = device.Id()?;
    let name = device.Name()?.to_string_lossy();
    let device = HidDevice::FromIdAsync(&id, FileAccessMode::Read)?;
    let Some(device) = device.await.extract_null()? else {
        return Ok(None);
    };
    Ok(Some(DeviceInfo {
        id: DeviceId::UncPath(id),
        name,
        product_id: device.ProductId()?,
        vendor_id: device.VendorId()?,
        usage_id: device.UsageId()?,
        usage_page: device.UsagePage()?,
        // Not supported
        serial_number: None,
    }))
}

pub struct InputReceiver {
    _backend: WinRtBackend,
    device: HidDevice,
    buffer: Receiver<HidInputReport>,
    token: i64
}

impl InputReceiver {
    fn new(backend: &WinRtBackend, id: &HSTRING, device: HidDevice) -> HidResult<Self> {
        let (sender, receiver) = async_channel::bounded(64);
        backend.get_watcher_context()?.mutex.lock().unwrap().push((id.clone(), sender.clone()));
        let token = device.InputReportReceived(&TypedEventHandler::new(move |_, args: Ref<HidInputReportReceivedEventArgs>| {
            debug!("{:?}", args.as_ref());
            if let Some(args) = args.as_ref() {
                let msg = args.Report()?;
                if let Err(SendError(_)) = sender.force_send(msg) {
                    log::trace!("Dropping previous input report because the queue is full");
                }
            }
            Ok(())
        }))?;
        Ok(Self { _backend: backend.clone(), device, buffer: receiver, token })
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
        let report = self.buffer
            .recv()
            .await
            .map_err(|_| HidError::Disconnected)?;
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


