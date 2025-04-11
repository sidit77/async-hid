mod utils;

use std::sync::{Arc, Mutex, PoisonError};
use std::sync::atomic::{AtomicU64, Ordering};
use async_channel::{Receiver, Sender, WeakSender};
use futures_lite::{StreamExt};
use log::{debug, trace};
use once_cell::sync::OnceCell;
use windows::core::{h, Ref, HRESULT, HSTRING};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher};
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

#[derive(Default)]
struct DeviceWatcherContext {
    next_id: AtomicU64,
    active_readers: Mutex<Vec<(u64, HSTRING, WeakSender<HidInputReport>)>>
}

#[derive(Default, Clone)]
pub struct WinRtBackend {
    context: Arc<DeviceWatcherContext>,
    watcher: Arc<OnceCell<DeviceWatcher>>
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
    backend: WinRtBackend,
    device: HidDevice,
    buffer: Receiver<HidInputReport>,
    token: i64,
    watcher_registration: u64
}

impl InputReceiver {
    fn new(backend: &WinRtBackend, id: &HSTRING, device: HidDevice) -> HidResult<Self> {
        let (sender, receiver) = async_channel::bounded(64);
        let watcher_registration = backend.register_active_reader(id.clone(), &sender)?;
        let token = device.InputReportReceived(&TypedEventHandler::new(move |_, args: Ref<HidInputReportReceivedEventArgs>| {
            debug!("{:?}", args.as_ref());
            if let Some(args) = args.as_ref() {
                let msg = args.Report()?;
                let _ = sender.force_send(msg);
            }
            Ok(())
        }))?;
        Ok(Self { backend: backend.clone(), device, buffer: receiver, token, watcher_registration })
    }
    
}

impl Drop for InputReceiver {
    fn drop(&mut self) {
        self.device
            .RemoveInputReportReceived(self.token)
            .unwrap_or_else(|err| log::warn!("Failed to unregister input report callback\n\t{err:?}"));
        self.backend
            .unregister_active_reader(self.watcher_registration);
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

impl WinRtBackend {

    fn initialize_watcher(&self) -> HidResult<()> {
        let _initialized = self.watcher.get_or_try_init(|| {
            let watcher = DeviceInformation::CreateWatcherAqsFilter(DEVICE_SELECTOR)?;

            watcher.Removed(&TypedEventHandler::new({
                let ctx = self.context.clone();
                move |_, info: Ref<DeviceInformationUpdate>| {
                    let info = info.ok()?;
                    let id = info.Id()?;
                    trace!("device removed: {:?}", id);
                    ctx
                        .active_readers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .retain(|(reg, rid, channel)| match rid == &id {
                            true => {
                                if let Some(channel) = channel.upgrade() {
                                    trace!("Force close channel of reader {}", reg);
                                    channel.close();
                                }
                                false
                            }
                            false => true
                        });
                    Ok(())
                }
            }))?;

            debug!("Starting device watcher");
            watcher.Start()?;

            Ok::<_, HidError>(watcher)
        })?;

        Ok(())
    }

    fn register_active_reader(&self, id: HSTRING, sender: &Sender<HidInputReport>) -> HidResult<u64> {
        self.initialize_watcher()?;
        let registration = self
            .context
            .next_id
            .fetch_add(1, Ordering::Relaxed);
        let mut readers = self.context
            .active_readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        readers.push((registration, id, sender.downgrade()));
        trace!("Registered active reader with device watcher (id: {}, number of registered readers: {})", registration, readers.len());
        Ok(registration)
    }

    fn unregister_active_reader(&self, registration: u64) {
        let mut readers = self.context
            .active_readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let count = readers.len();
        readers.retain(|(id, _, _)| *id != registration);
        if readers.len() == count {
            trace!("Reader {} was already removed from the device watcher", registration);
        } else {
            trace!("Unregistered reader {} from the device watcher", registration);
        }
    }

}
