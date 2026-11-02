use std::fmt::{Debug, Formatter};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use log::{debug, error, trace, warn};
use windows::core::HRESULT;
use windows::Win32::Foundation::{CloseHandle, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_NOT_FOUND, STATUS_PENDING};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Threading::CreateEventW;
use windows::Win32::System::IO::{CancelIoEx, DeviceIoControl, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED};

use crate::backend::win32::device::Device;
use crate::backend::win32::waiter::HandleWaiter;
use crate::HidResult;

#[derive(Debug, Default)]
pub struct Readable;

#[derive(Debug, Default)]
pub struct Writable;

/// Feature reads and writes share one buffer and one OVERLAPPED, so the
/// handle remembers which kind of operation is pending.
#[derive(Debug, Default)]
pub struct Feature {
    /// The report ID of the pending read, `None` while a write is pending.
    pending_read: Option<u8>
}

impl Feature {
    /// Whether a read of `report_id` may take over the result of the
    /// operation that a dropped call left pending.
    fn can_take_over(&self, report_id: u8) -> bool {
        self.pending_read == Some(report_id)
    }
}

pub struct IoBuffer<T> {
    device: Arc<Device>,
    buffer: ManuallyDrop<Box<[u8]>>,
    overlapped: ManuallyDrop<Overlapped>,
    pending: bool,
    state: T
}

impl<T> Debug for IoBuffer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IoBuffer")
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl<T: Default> IoBuffer<T> {
    pub fn new(device: Arc<Device>, size: usize) -> HidResult<Self> {
        let overlapped = Overlapped::new()?;
        let buffer = vec![0; size].into_boxed_slice();
        Ok(IoBuffer {
            device,
            buffer: ManuallyDrop::new(buffer),
            overlapped: ManuallyDrop::new(overlapped),
            pending: false,
            state: T::default()
        })
    }
}

impl<T> IoBuffer<T> {
    async fn wait_for_write_to_complete(&mut self) -> HidResult<()> {
        if self.pending {
            loop {
                match self.get_result()? {
                    Some(size) => {
                        trace!("Completed write operation (transferred {} bytes)", size);
                        return Ok(());
                    }
                    None => self.overlapped.wait_for_completion().await?
                }
            }
        }
        Ok(())
    }

    /// Waits for an operation that a dropped call left pending and discards
    /// its result. Fails only if the operation may still be running, because
    /// its buffer can not be reused then.
    async fn finish_abandoned(&mut self) -> HidResult<()> {
        match self.wait_for_write_to_complete().await {
            Err(err) if self.pending => Err(err),
            Err(err) => {
                error!("Abandoned I/O operation failed: {err}");
                Ok(())
            }
            Ok(()) => Ok(())
        }
    }

    fn start_io<F>(&mut self, operation: F) -> HidResult<()>
    where
        F: FnOnce(&Device, &mut [u8], &mut Overlapped) -> windows::core::Result<()>
    {
        assert!(!self.pending, "I/O operation already pending");
        let r = operation(&self.device, &mut self.buffer, &mut self.overlapped);
        self.pending = true;
        if let Err(e) = r && e.code() != HRESULT::from_win32(ERROR_IO_PENDING.0) {
            self.cancel_io().expect("Failed to cancel I/O operation");
            return Err(e.into())
        }
        Ok(())
    }

    fn cancel_io(&mut self) -> HidResult<()> {
        match unsafe { CancelIoEx(self.device.handle(), Some(self.overlapped.as_raw())) } {
            Ok(()) => Ok(()),
            Err(err) if err.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
            Err(err) => Err(err.into())
        }.inspect(|_| self.pending = false)
    }

    fn get_result(&mut self) -> HidResult<Option<usize>> {
        let mut bytes_transferred = 0;
        let result = unsafe { GetOverlappedResult(self.device.handle(), self.overlapped.as_raw(), &mut bytes_transferred, false) };
        match result {
            Ok(()) => {
                self.pending = false;
                Ok(Some(bytes_transferred as usize))
            }
            Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_INCOMPLETE.0) => Ok(None),
            Err(err) => {
                self.pending = false;
                Err(err.into())
            }
        }
    }
}

/// How long dropping a handle waits for a canceled operation to complete.
/// After that the buffer is leaked, so that a driver that does not honor the
/// cancellation can not block the dropping thread indefinitely.
const CANCEL_TIMEOUT_MS: u32 = 1000;

impl<T> IoBuffer<T> {
    /// Cancels the pending operation and waits until it has completed, for
    /// at most [CANCEL_TIMEOUT_MS].
    ///
    /// `CancelIoEx` only requests the cancellation. The buffer and the
    /// OVERLAPPED of the operation must stay valid until it has completed,
    /// normally or canceled. Returns `false` if the completion could not be
    /// confirmed, in which case the operation stays marked as pending.
    fn cancel_and_wait(&mut self) -> bool {
        trace!("Canceling pending I/O operation");
        // A future that was leaked instead of dropped leaves its wait on the
        // auto-reset event registered, and that wait could take the signal
        // that the wait below needs.
        if let Err(err) = self.overlapped.cancel_wait() {
            error!("Failed to unregister the wait of the pending I/O operation: {err}");
            return false;
        }
        if let Err(err) = self.cancel_io() {
            error!("Failed to cancel I/O operation: {err}");
            return false;
        }
        // GetOverlappedResultEx with a timeout was seen to wait for the event
        // although the operation had completed (Windows 11 ARM64, build
        // 26300), and the signal of the auto-reset event may already be gone,
        // taken by the wait of a future that was dropped meanwhile.
        if self.overlapped.is_pending() {
            // A timeout is no error by itself: whether the operation has
            // completed is decided by is_pending below.
            let mut bytes_transferred = 0;
            let result = unsafe {
                GetOverlappedResultEx(
                    self.device.handle(),
                    self.overlapped.as_raw(),
                    &mut bytes_transferred,
                    CANCEL_TIMEOUT_MS,
                    false
                )
            };
            trace!("Waited for the canceled I/O operation: {result:?}");
        }
        self.pending = self.overlapped.is_pending();
        !self.pending
    }
}

impl<T> Drop for IoBuffer<T> {
    fn drop(&mut self) {
        if self.pending && !self.cancel_and_wait() {
            // The operation may still write to the buffer and the OVERLAPPED,
            // so both are leaked to keep them valid.
            error!("Leaking the buffer of an I/O operation that did not complete");
            return;
        }
        // SAFETY: Without a pending operation nothing but `self` refers to the
        // buffer and the OVERLAPPED, and neither is used after this.
        unsafe {
            ManuallyDrop::drop(&mut self.buffer);
            ManuallyDrop::drop(&mut self.overlapped);
        }
    }
}

impl IoBuffer<Readable> {
    fn start_read(&mut self) -> HidResult<()> {
        self.start_io(|device, buffer, overlapped| unsafe {
            trace!("Starting new read operation");
            ReadFile(device.handle(), Some(buffer), None, Some(overlapped.as_raw_mut()))
        })
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> HidResult<usize> {
        loop {
            match self.pending {
                false => self.start_read()?,
                true => match self.get_result()? {
                    Some(size) => {
                        trace!("Completed read operation (retrieved {} bytes)", size);
                        let mut data = &self.buffer[..size];
                        if data[0] == 0x0 {
                            data = &data[1..];
                        }
                        let mut copy_len = data.len();
                        if copy_len > buf.len() {
                            debug!(
                                "Input report ({}) is larger than the provided buffer ({}), truncating data",
                                copy_len,
                                buf.len()
                            );
                            copy_len = buf.len();
                        }
                        buf[..copy_len].copy_from_slice(&data[..copy_len]);
                        return Ok(copy_len);
                    }
                    None => self.overlapped.wait_for_completion().await?
                }
            }
        }
    }
}

impl IoBuffer<Writable> {
    fn start_write(&mut self) -> HidResult<()> {
        self.start_io(|device, buffer, overlapped| unsafe {
            trace!("Starting new write operation");
            WriteFile(device.handle(), Some(buffer), None, Some(overlapped.as_raw_mut()))
        })
    }

    pub async fn write(&mut self, data: &[u8]) -> HidResult<()> {
        self.finish_abandoned().await?;

        trace!("Filling write buffer with data");
        let mut data_size = data.len();
        if data_size > self.buffer.len() {
            debug!(
                "Data size ({}) exceeds maximum buffer size ({}), truncating data",
                data_size,
                self.buffer.len()
            );
            data_size = self.buffer.len();
        }
        self.buffer[data_size..].fill(0);
        self.buffer[..data_size].copy_from_slice(&data[..data_size]);

        self.start_write()?;
        self.wait_for_write_to_complete().await?;
        Ok(())
    }
}

const IOCTL_HID_GET_FEATURE: u32 = 0x000B0192;
const IOCTL_HID_SET_FEATURE: u32 = 0x000B0191;

impl IoBuffer<Feature> {
    fn start_feature_read(&mut self, report_id: u8) -> HidResult<()> {
        self.buffer[0] = report_id;
        self.state.pending_read = Some(report_id);
        self.start_io(|device, buffer, overlapped| unsafe {
            trace!("Starting feature read for report ID {}", report_id);
            DeviceIoControl(
                device.handle(),
                IOCTL_HID_GET_FEATURE,
                None,
                0,
                Some(buffer.as_mut_ptr() as *mut std::ffi::c_void),
                buffer.len() as u32,
                None,
                Some(overlapped.as_raw_mut())
            )
        })
    }

    fn start_feature_write(&mut self) -> HidResult<()> {
        self.state.pending_read = None;
        self.start_io(|device, buffer, overlapped| unsafe {
            trace!("Starting feature write operation");
            DeviceIoControl(
                device.handle(),
                IOCTL_HID_SET_FEATURE,
                Some(buffer.as_ptr() as *const std::ffi::c_void),
                buffer.len() as u32,
                None,
                0,
                None,
                Some(overlapped.as_raw_mut())
            )
        })
    }

    pub async fn read_feature_report(&mut self, buf: &mut [u8]) -> HidResult<usize> {
        let report_id = buf[0];

        // A read of the same report takes over the result of a read that a
        // dropped call left pending. Any other pending operation is finished
        // first and its result discarded.
        if self.pending && !self.state.can_take_over(report_id) {
            self.finish_abandoned().await?;
        }

        loop {
            match self.pending {
                false => self.start_feature_read(report_id)?,
                true => match self.get_result()? {
                    Some(size) => {
                        trace!("Completed feature read operation (retrieved {} bytes)", size);
                        let data = &self.buffer[..size];
                        let copy_len = std::cmp::min(data.len(), buf.len());
                        buf[..copy_len].copy_from_slice(&data[..copy_len]);
                        return Ok(copy_len);
                    }
                    None => self.overlapped.wait_for_completion().await?
                }
            }
        }
    }

    pub async fn write_feature_report(&mut self, buf: &[u8]) -> HidResult<()> {
        self.finish_abandoned().await?;

        trace!("Filling feature write buffer with data");
        let mut data_size = buf.len();
        if data_size > self.buffer.len() {
            debug!(
                "Data size ({}) exceeds maximum buffer size ({}), truncating data",
                data_size,
                self.buffer.len()
            );
            data_size = self.buffer.len();
        }
        self.buffer[data_size..].fill(0);
        self.buffer[..data_size].copy_from_slice(&buf[..data_size]);

        self.start_feature_write()?;
        self.wait_for_write_to_complete().await?;
        Ok(())
    }
}

struct Overlapped {
    inner: *mut OVERLAPPED,
    waiter: HandleWaiter
}

impl Overlapped {
    pub fn new() -> HidResult<Self> {
        let event = unsafe { CreateEventW(None, false, false, None)? };
        Ok(Overlapped {
            inner: Box::into_raw(Box::new(OVERLAPPED {
                hEvent: event,
                ..Default::default()
            })),
            waiter: HandleWaiter::new(event)
        })
    }

    pub async fn wait_for_completion(&mut self) -> HidResult<()> {
        self.waiter.wait().await
    }

    pub fn as_raw(&self) -> *const OVERLAPPED {
        self.inner
    }

    pub fn as_raw_mut(&mut self) -> *mut OVERLAPPED {
        self.inner
    }

    /// Whether the operation that last used this OVERLAPPED is still in
    /// progress, as tested by the `HasOverlappedIoCompleted` macro.
    pub fn is_pending(&self) -> bool {
        // SAFETY: `inner` is a live, aligned allocation owned by `self`. The
        // system writes `Internal` when the operation completes, so it is read
        // atomically. As in the macro, only the low 32 bits hold the status.
        let internal = unsafe { AtomicUsize::from_ptr(std::ptr::addr_of_mut!((*self.inner).Internal)) }.load(Ordering::Acquire);
        internal as u32 == STATUS_PENDING.0 as u32
    }

    /// Removes a wait on the event that a leaked future left registered.
    pub fn cancel_wait(&mut self) -> HidResult<()> {
        self.waiter.cancel()
    }
}

unsafe impl Send for Overlapped {}
unsafe impl Sync for Overlapped {}

impl Drop for Overlapped {
    fn drop(&mut self) {
        let inner = unsafe { Box::from_raw(self.inner) };
        // The event must not be closed while a wait on it is registered.
        if let Err(err) = self.waiter.cancel() {
            warn!("Leaking the event of a wait that could not be unregistered: {err}");
            return;
        }
        unsafe { CloseHandle(inner.hEvent).unwrap_or_else(|err| warn!("Failed to close handle: {err}")) };
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{remove_file, File};
    use std::path::PathBuf;

    use futures_lite::future::{block_on, poll_once};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE};
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount, WaitForSingleObject};

    use super::*;

    fn handle_count() -> i64 {
        let mut count = 0;
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }.expect("Failed to get the handle count");
        count.into()
    }

    /// A plain file opened the way a HID device is opened, with FILE_FLAG_OVERLAPPED.
    fn file_device(path: &PathBuf) -> Arc<Device> {
        File::create(path).expect("Failed to create the test file");
        let path = HSTRING::from(path.as_path());
        Arc::new(Device::open(PCWSTR(path.as_ptr()), true, true).expect("Failed to open the test file"))
    }

    #[test]
    fn dropping_an_idle_buffer_releases_its_event() {
        const BUFFERS: i64 = 64;
        let path = std::env::temp_dir().join(format!("async-hid-idle-buffer-{}", std::process::id()));
        let device = file_device(&path);

        let before = handle_count();
        for _ in 0..BUFFERS {
            drop(IoBuffer::<Readable>::new(device.clone(), 16).expect("Failed to create the buffer"));
        }
        let after = handle_count();

        drop(device);
        let _ = remove_file(&path);
        // Every buffer owns one event handle, so a leak grows the count by BUFFERS.
        assert!(after - before < BUFFERS / 2, "handle count grew from {before} to {after}");
    }

    /// A read from a named pipe that nobody writes to stays pending. The
    /// cancellation of such a read usually completes at once, so this checks
    /// what cancel_and_wait reports and leaves behind rather than the wait.
    #[test]
    fn cancel_and_wait_returns_once_the_read_has_completed() {
        let name = HSTRING::from(format!(r"\\.\pipe\async-hid-pending-read-{}", std::process::id()));
        let server = unsafe { CreateNamedPipeW(&name, PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED, PIPE_TYPE_BYTE, 1, 64, 64, 0, None) };
        assert!(!server.is_invalid(), "Failed to create the pipe");
        let device = Arc::new(Device::open(PCWSTR(name.as_ptr()), true, true).expect("Failed to open the pipe"));
        let mut buffer = IoBuffer::<Readable>::new(device, 16).expect("Failed to create the buffer");

        let mut report = [0u8; 16];
        assert!(block_on(poll_once(buffer.read(&mut report))).is_none(), "the read must be pending");
        assert!(buffer.pending && buffer.overlapped.is_pending());

        assert!(buffer.cancel_and_wait(), "the canceled read must have completed");
        assert!(!buffer.pending && !buffer.overlapped.is_pending());

        drop(buffer);
        unsafe { CloseHandle(server) }.expect("Failed to close the pipe");
    }

    /// A read that completed while its future was being dropped: the thread
    /// pool wait took the signal of the auto-reset event, but nobody fetched
    /// the result. Dropping the buffer must not wait for that signal again.
    #[test]
    fn cancel_and_wait_does_not_wait_for_a_completed_read() {
        let name = HSTRING::from(format!(r"\\.\pipe\async-hid-completed-read-{}", std::process::id()));
        let server = unsafe { CreateNamedPipeW(&name, PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE, 1, 64, 64, 0, None) };
        assert!(!server.is_invalid(), "Failed to create the pipe");
        let device = Arc::new(Device::open(PCWSTR(name.as_ptr()), true, true).expect("Failed to open the pipe"));
        let mut buffer = IoBuffer::<Readable>::new(device, 16).expect("Failed to create the buffer");

        let mut report = [0u8; 16];
        assert!(block_on(poll_once(buffer.read(&mut report))).is_none(), "the read must be pending");
        let mut written = 0;
        unsafe { WriteFile(server, Some(&[0x01, 0x02]), Some(&mut written), None) }.expect("Failed to write to the pipe");
        let event = unsafe { (*buffer.overlapped.as_raw()).hEvent };
        assert_eq!(unsafe { WaitForSingleObject(event, 1000) }, WAIT_OBJECT_0, "the read must complete");
        assert!(buffer.pending && !buffer.overlapped.is_pending());

        let started = std::time::Instant::now();
        assert!(buffer.cancel_and_wait(), "the read has completed");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "waited {:?}",
            started.elapsed()
        );

        drop(buffer);
        unsafe { CloseHandle(server) }.expect("Failed to close the pipe");
    }

    #[test]
    fn only_a_read_of_the_same_report_takes_over_a_pending_read() {
        let feature = Feature { pending_read: Some(0x05) };
        assert!(feature.can_take_over(0x05));
        assert!(!feature.can_take_over(0x06));
    }

    #[test]
    fn no_read_takes_over_a_pending_write() {
        let feature = Feature { pending_read: None };
        assert!(!feature.can_take_over(0x00));
        assert!(!feature.can_take_over(0x05));
    }
}

/*
TODO: Figure out how to make the buffer logic mockable so that we can write tests against it
#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::{ERROR_GEN_FAILURE, ERROR_IO_INCOMPLETE, ERROR_NO_SUCH_DEVICE, WIN32_ERROR};

    use super::*;

    fn win32_error(code: WIN32_ERROR) -> windows::core::Error {
        windows::core::Error::from_hresult(HRESULT::from_win32(code.0))
    }

    #[test]
    fn incomplete_result_keeps_operation_pending() {
        let mut pending = true;
        let result = classify_io_result(&mut pending, Err(win32_error(ERROR_IO_INCOMPLETE)), 0);

        assert!(matches!(result, Ok(None)));
        assert!(pending);
    }

    #[test]
    fn terminal_errors_clear_operation_pending() {
        for error in [ERROR_NO_SUCH_DEVICE, ERROR_GEN_FAILURE] {
            let mut pending = true;
            let result = classify_io_result(&mut pending, Err(win32_error(error)), 0);

            assert!(result.is_err());
            assert!(!pending);
        }
    }

    #[test]
    fn successful_result_clears_operation_pending() {
        let mut pending = true;
        let result = classify_io_result(&mut pending, Ok(()), 17);

        assert!(matches!(result, Ok(Some(17))));
        assert!(!pending);
    }
}
 */
