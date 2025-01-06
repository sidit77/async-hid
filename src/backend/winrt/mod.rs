
mod device;

use std::fmt::Display;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_lite::{Stream, StreamExt};
use windows::core::{h, HRESULT, HSTRING, PCWSTR};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationCollection};
use windows::Storage::FileAccessMode;
use windows::Win32::Devices::HumanInterfaceDevice::HidD_SetNumInputBuffers;
use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResultEx, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, INFINITE};
use crate::error::{ErrorSource, HidResult};
use crate::{ensure, AccessMode, DeviceInfo, HidError, SerialNumberExt};
use crate::backend::winrt::device::Device;

const DEVICE_SELECTOR: &HSTRING = h!(
    r#"System.Devices.InterfaceClassGuid:="{4D1E55B2-F16F-11CF-88CB-001111000030}" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True"#
);

pub async fn enumerate() -> HidResult<impl Stream<Item = DeviceInfo> + Unpin + Send> {
    //let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
    //    .await?
    //    .into_iter()
    //    .map(get_device_information)
    //    .collect::<FuturesUnordered<_>>()
    //    .filter_map(|info| ready(info.ok()))
    //    .collect()
    //    .await;
    let devices = DeviceInformation::FindAllAsyncAqsFilter(DEVICE_SELECTOR)?
        .await?;
    let devices = DeviceInformationSteam::from(devices)
        .map(get_device_information)
        .filter_map(|r| {
            r.map_err(|e| log::trace!("Failed to query device information\n\tbecause {e:?}"))
                .ok()
        });
    //.collect()
    //.await;
    Ok(devices)
}

impl SerialNumberExt for DeviceInfo {
    fn serial_number(&self) -> Option<&str> {
        self.private_data
            .serial_number
            .as_ref()
            .map(String::as_str)
    }
}

fn get_device_information(device: DeviceInformation) -> HidResult<DeviceInfo> {
    let id = device.Id()?;
    let name = device.Name()?.to_string_lossy();
    let device = Device::open(PCWSTR(id.as_ptr()), None)?;
    let attribs = device.attributes()?;
    let caps = device.preparsed_data()?.caps()?;
    let serial_number = device.serial_number().ok();
    Ok(DeviceInfo {
        id: HashableHSTRING(id).into(),
        name,
        product_id: attribs.ProductID,
        vendor_id: attribs.VendorID,
        usage_id: caps.Usage,
        usage_page: caps.UsagePage,
        private_data: BackendPrivateData {
            serial_number
        }
    })
}

/*
#[derive(Debug, Clone)]
struct InputReceiver {
    buffer: Receiver<HidInputReport>,
    token: EventRegistrationToken
}

impl InputReceiver {
    fn new(device: &HidDevice) -> HidResult<Self> {
        let (sender, receiver) = flume::bounded(64);
        let drain = receiver.clone();
        let token = device.InputReportReceived(&TypedEventHandler::new(move |_, args: &Option<HidInputReportReceivedEventArgs>| {
            if let Some(args) = args {
                let mut msg = args.Report()?;
                while let Err(TrySendError::Full(ret)) = sender.try_send(msg) {
                    log::trace!("Dropping previous input report because the queue is full");
                    let _ = drain.try_recv();
                    msg = ret;
                }
            }
            Ok(())
        }))?;
        Ok(Self { buffer: receiver, token })
    }

    async fn recv_async(&self) -> HidInputReport {
        self.buffer
            .recv_async()
            .await
            .expect("Input report handler got dropped unexpectedly")
    }

    fn stop(self, device: &HidDevice) -> HidResult<()> {
        Ok(device.RemoveInputReportReceived(self.token)?)
    }
}

 */

#[derive(Debug)]
pub struct BackendDevice {
    device: Device,
    write_buffer_size: usize,
    read_buffer_size: usize,
}

impl Drop for BackendDevice {
    fn drop(&mut self) {
        //if let Some(input) = self.input.take() {
        //    input
        //        .stop(&self.device)
        //        .unwrap_or_else(|err| log::warn!("Failed to unregister input report callback\n\t{err:?}"));
        //}
    }
}

pub async fn open(id: &BackendDeviceId, mode: AccessMode) -> HidResult<BackendDevice> {
    let device = Device::open(PCWSTR(id.as_ptr()), Some(mode))?;

    unsafe {
        HidD_SetNumInputBuffers(device.handle(), 64).ok()?;
    }
    let caps = device.preparsed_data()?.caps()?;

    Ok(BackendDevice {
        device,
        write_buffer_size: caps.OutputReportByteLength as usize,
        read_buffer_size: caps.InputReportByteLength as usize,
    })
}

impl BackendDevice {
    pub async fn read_input_report(&self, buf: &mut [u8]) -> HidResult<usize> {
        //let report = self
        //    .input
        //    .as_ref()
        //    .expect("Reading is disabled")
        //    .recv_async()
        //    .await;
        //let buffer = report.Data()?;
        //let buffer = buffer.as_slice()?;
        //ensure!(!buffer.is_empty(), HidError::custom("Input report is empty"));
        //let size = buf.len().min(buffer.len());
        //let start = if buffer[0] == 0x0 { 1 } else { 0 };
        //buf[..(size - start)].copy_from_slice(&buffer[start..size]);

        //Ok(size - start)

        let mut bytes_read = 0;
        let mut rb = vec![0u8; self.read_buffer_size];

        //TODO make sure the handle does not leak
        let event = unsafe { CreateEventW(None, false, false, None)? };
        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = event;

        let res = unsafe {
            ReadFile(
                self.device.handle(),
                Some(&mut rb),
                Some(&mut bytes_read),
                Some(&mut overlapped)
            )
        };

        match res {
            Ok(()) => {},
            Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
                unsafe {
                    GetOverlappedResultEx(
                        self.device.handle(),
                        &mut overlapped,
                        &mut bytes_read,
                        INFINITE,
                        false,
                    )?;
                }
            },
            Err(err) => {
                unsafe { CancelIoEx(self.device.handle(), Some(&mut overlapped))? };
                return Err(err.into())
            }
        }

        unsafe { CloseHandle(event)?; }

        let copy_len;
        if rb[0] == 0x0 {
            bytes_read -= 1;
            copy_len = usize::min(bytes_read as usize, buf.len());
            buf[..copy_len].copy_from_slice(&rb[1..(1 + copy_len)]);
        } else {
            copy_len = usize::min(bytes_read as usize, buf.len());
            buf[..copy_len].copy_from_slice(&rb[0..copy_len]);
        }

        Ok(copy_len)
    }

    pub async fn write_output_report(&self, buf: &[u8]) -> HidResult<()> {
        ensure!(!buf.is_empty(), HidError::zero_sized_data());
        let mut wb = vec![0u8; self.write_buffer_size];

        let data_size = buf.len().min(wb.len());
        wb.fill(0);
        wb[..data_size].copy_from_slice(&buf[..data_size]);

        //TODO make sure the handle does not leak
        let event = unsafe { CreateEventW(None, false, false, None)? };
        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = event;
        let res = unsafe {
            WriteFile(
                self.device.handle(),
                Some(&mut wb),
                None,
                Some(&mut overlapped)
            )
        };

        match res {
            Ok(()) => {}
            Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
                let mut bytes_written = 0;
                unsafe {
                    GetOverlappedResultEx(
                        self.device.handle(),
                        &mut overlapped,
                        &mut bytes_written,
                        INFINITE,
                        false,
                    )?;
                }
            },
            Err(err) => return Err(err.into())
        }

        unsafe {
            CloseHandle(event)?;
        }



        //let report = self.device.CreateOutputReport()?;
//
        //{
        //    let mut buffer = report.Data()?;
        //    ensure!(buffer.Length()? as usize >= buf.len(), HidError::custom("Output report is too large"));
        //    let (buffer, remainder) = buffer.as_mut_slice()?.split_at_mut(buf.len());
        //    buffer.copy_from_slice(buf);
        //    remainder.fill(0);
        //}
//
        //self.device.SendOutputReportAsync(&report)?.await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BackendPrivateData {
    serial_number: Option<String>
}

/// Wrapper type for HSTRING to add Hash implementation
///
/// windows-rs has a built-in Hash HSTRING implementation after version 0.55.0 (introduced by this PR https://github.com/microsoft/windows-rs/pull/2924/files)
/// Though, a direct upgrade to the newer windows-rs versions would require further work due to API and functionality changes
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HashableHSTRING(HSTRING);

impl Display for HashableHSTRING {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for HashableHSTRING {
    type Target = HSTRING;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HashableHSTRING {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Hash for HashableHSTRING {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.0.as_wide().hash(hasher)
    }
}

pub type BackendDeviceId = HashableHSTRING;
pub type BackendError = windows::core::Error;

impl From<BackendError> for ErrorSource {
    fn from(value: BackendError) -> Self {
        ErrorSource::PlatformSpecific(value)
    }
}

impl From<AccessMode> for FileAccessMode {
    fn from(value: AccessMode) -> Self {
        match value {
            AccessMode::Read => FileAccessMode::Read,
            AccessMode::Write => FileAccessMode::ReadWrite,
            AccessMode::ReadWrite => FileAccessMode::ReadWrite
        }
    }
}

struct DeviceInformationSteam {
    devices: DeviceInformationCollection,
    index: u32
}

impl From<DeviceInformationCollection> for DeviceInformationSteam {
    fn from(value: DeviceInformationCollection) -> Self {
        Self {
            devices: value,
            index: 0,
        }
    }
}

impl Stream for DeviceInformationSteam {
    type Item = DeviceInformation;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let current = self.index;
        self.index += 1;
        Poll::Ready(self.devices.GetAt(current).ok())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self
            .devices
            .Size()
            .expect("Failed to get the length of the collection") - self.index) as usize;
        (remaining, Some(remaining))
    }
}
