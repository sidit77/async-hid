use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::mem::{forget, take};
use std::sync::{Arc};
use log::{debug, error, trace, warn};
use windows::core::HRESULT;
use windows::Win32::Foundation::{CloseHandle, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_NOT_FOUND};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::CreateEventW;
use crate::backend::win32::device::Device;
use crate::backend::win32::waiter::WaitableHandleFuture;
use crate::HidResult;

#[derive(Debug)]
pub struct Readable;

#[derive(Debug)]
pub struct Writable;

pub struct IoBuffer<T> {
    device: Arc<Device>,
    buffer: Box<[u8]>,
    overlapped: Box<Overlapped>,
    pending: bool,
    _marker: PhantomData<T>,
}

impl<T> Debug for IoBuffer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IoBuffer")
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl<T> IoBuffer<T> {
    pub fn new(device: Arc<Device>, size: usize) -> HidResult<Self> {
        Ok(IoBuffer {
            device,
            buffer: vec![0; size].into_boxed_slice(),
            overlapped: Box::new(Overlapped::new()?),
            pending: false,
            _marker: PhantomData,
        })
    }

    fn start_io<F>(&mut self, operation: F) -> HidResult<()>
        where F: FnOnce(&Device, &mut [u8], &mut Overlapped) -> windows::core::Result<()>
    {
        assert!(!self.pending, "I/O operation already pending");
        let result = operation(&self.device, &mut self.buffer, self.overlapped.as_mut());
        match result {
            Ok(_) => { self.pending = true; }
            Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
                self.pending = true;
            }
            Err(err) => {
                if let Err(err) = self.cancel_io() {
                    self.pending = true;
                    panic!("Failed to cancel I/O operation: {:?}", err);
                } else {
                    self.pending = false;
                }
                return Err(err.into());
            }
        }
        Ok(())
    }

    fn cancel_io(&mut self) -> HidResult<()> {
        match unsafe { CancelIoEx(self.device.handle(), Some(self.overlapped.as_raw())) } {
            Ok(()) => Ok(()),
            Err(err) if err.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
            Err(err) => Err(err.into())
        }
    }

    fn get_result(&mut self) -> HidResult<Option<usize>> {
        let mut bytes_transferred = 0;
        let result = unsafe {
            GetOverlappedResult(
                self.device.handle(),
                self.overlapped.as_raw(),
                &mut bytes_transferred,
                false
            )
        };
        match result {
            Ok(()) => Ok(Some(bytes_transferred as usize)),
            Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_INCOMPLETE.0) => Ok(None),
            Err(err) => Err(err.into())
        }
    }

}

impl<T> Drop for IoBuffer<T> {
    fn drop(&mut self) {
        if self.pending {
            trace!("Canceling pending I/O operation");
            if let Err(err) = self.cancel_io() {
                // SAFETY: If canceling the I/O operation fails, the buffer and overlapped structures are leaked before we panic to make sure they stay valid even after `Self` gets freed.
                forget(take(&mut self.buffer));
                forget(take(&mut self.overlapped));
                panic!("Failed to cancel I/O operation: {:?}", err);
            }
        }
    }
}

impl IoBuffer<Readable> {

    fn start_read(&mut self) -> HidResult<()> {
        self.start_io(|device, buffer, overlapped| unsafe {
            trace!("Starting new read operation");
            ReadFile(
                device.handle(),
                Some(buffer),
                None,
                Some(overlapped.as_raw_mut()),
            )
        })
    }

    pub async fn read(&mut self, buf: &mut[u8]) -> HidResult<usize> {
        loop {
            match self.pending {
                false => self.start_read()?,
                true => match self.get_result()?{
                    Some(size) => {
                        trace!("Completed read operation (retrieved {} bytes)", size);
                        let mut data = &self.buffer[..size];
                        if data[0] == 0x0 {
                            data = &data[1..];
                        }
                        let mut copy_len = data.len();
                        if copy_len > buf.len() {
                            debug!("Input report ({}) is larger than the provided buffer ({}), truncating data", copy_len, buf.len());
                            copy_len = buf.len();
                        }
                        buf[..copy_len].copy_from_slice(&data[..copy_len]);
                        self.pending = false;
                        return Ok(copy_len);
                    },
                    None => self.overlapped.wait_for_completion().await?,
                }
            }
        }
    }
}

impl IoBuffer<Writable> {

     async fn wait_for_write_to_complete(&mut self) -> HidResult<()> {
        if self.pending {
            loop {
                match self.get_result()? {
                    Some(size) => {
                        trace!("Completed write operation (transferred {} bytes)", size);
                        self.pending = false;
                        return Ok(());
                    },
                    None => self.overlapped.wait_for_completion().await?,
                }
            }
        }
        Ok(())
    }

    fn start_write(&mut self) -> HidResult<()> {
        self.start_io(|device, buffer, overlapped| unsafe {
            trace!("Starting new write operation");
            WriteFile(
                device.handle(),
                Some(buffer),
                None,
                Some(overlapped.as_raw_mut()),
            )
        })
    }

    pub async fn write(&mut self, data: &[u8]) -> HidResult<()> {
        self.wait_for_write_to_complete().await.unwrap_or_else(|err| error!("Abandoned write failed: {err}"));

        trace!("Filling write buffer with data");
        let mut data_size = data.len();
        if data_size > self.buffer.len() {
            debug!("Data size ({}) exceeds maximum buffer size ({}), truncating data", data_size, self.buffer.len());
            data_size = self.buffer.len();
        }
        self.buffer[data_size..].fill(0);
        self.buffer[..data_size].copy_from_slice(&data[..data_size]);

        self.start_write()?;
        self.wait_for_write_to_complete().await?;
        Ok(())
    }

}

#[derive(Default)]
#[repr(transparent)]
struct Overlapped(OVERLAPPED);

impl Overlapped {
    pub fn new() -> HidResult<Self> {
        Ok(Overlapped(OVERLAPPED {
            hEvent: unsafe { CreateEventW(None, false, false, None)? },
            ..Default::default()
        }))
    }

    pub async fn wait_for_completion(&self) -> HidResult<()> {
        WaitableHandleFuture::new(self.0.hEvent).await
    }

    pub fn as_raw(&self) -> *const OVERLAPPED {
        &self.0
    }

    pub fn as_raw_mut(&mut self) -> *mut OVERLAPPED {
        &mut self.0
    }

}

unsafe impl Send for Overlapped {}
unsafe impl Sync for Overlapped {}

impl Drop for Overlapped {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0.hEvent).unwrap_or_else(|err| warn!("Failed to close handle: {err}")) };
    }
}