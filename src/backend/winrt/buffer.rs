use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::mem::{forget, take};
use std::sync::{Arc};
use log::{trace, warn};
use windows::core::HRESULT;
use windows::Win32::Foundation::{CloseHandle, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_NOT_FOUND, HANDLE};
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::CreateEventW;
use crate::backend::winrt::device::Device;
use crate::backend::winrt::waiter::wait_for_handle;
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

impl<T: Debug> Debug for IoBuffer<T> {
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
            if let Err(err) = unsafe { CancelIoEx(self.device.handle(), Some(self.overlapped.as_raw())) } {
                // SAFETY: If canceling the I/O operation fails, the buffer and overlapped structures are leaked before we panic to make sure they stay valid even after `Self` gets freed.
                forget(take(&mut self.buffer));
                forget(take(&mut self.overlapped));
                panic!("Failed to cancel I/O operation: {:?}", err);
            }
        }
    }
}

impl IoBuffer<Readable> {

    pub fn begin_read(&mut self) -> HidResult<()> {
        assert!(!self.pending, "I/O operation already pending");
        trace!("Starting new read operation");
        let result = unsafe {
            ReadFile(
                self.device.handle(),
                // SAFETY: The mutex around pending also acts as a lock for the buffer.
                Some(self.buffer.as_mut()),
                None,
                // SAFETY: [Overlapped] is marked as `repr(transparent)`
                Some(self.overlapped.as_raw_mut()),
            )
        };
        match result {
            Ok(_) => { self.pending = true; }
            Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {
                self.pending = true;
            }
            Err(err) => {
                match unsafe { CancelIoEx(self.device.handle(), Some(self.overlapped.as_raw())) } {
                    Ok(()) => {},
                    Err(err) if err.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => {},
                    Err(err) => {
                        // Prevent cleanup or reuse of the buffer and overlapped structures.
                        self.pending = true;
                        panic!("Failed to cancel I/O operation: {:?}", err);
                    }
                }
                self.pending = false;
                return Err(err.into());
            }
        }
        Ok(())
    }

    pub async fn read<F: FnOnce(&[u8])>(&mut self, callback: F) -> HidResult<()> {
        loop {
            match self.pending {
                false => self.begin_read()?,
                true => match self.get_result()?{
                    Some(size) => {
                        trace!("Completed read operation (retrieved {} bytes)", size);
                        callback(&self.buffer[..size]);
                        self.pending = false;
                        return Ok(());
                    },
                    None => wait_for_handle(self.overlapped.event_handle()).await?,
                }
            }
        }
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

    pub fn event_handle(&self) -> HANDLE {
        self.0.hEvent
    }

    pub fn as_raw(&self) -> *const OVERLAPPED {
        &self.0
    }

    pub fn as_raw_mut(&mut self) -> *mut OVERLAPPED {
        &mut self.0
    }

}

impl Drop for Overlapped {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0.hEvent).unwrap_or_else(|err| warn!("Failed to close handle: {err}")) };
    }
}