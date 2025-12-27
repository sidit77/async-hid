use std::ffi::c_void;
use std::future::{poll_fn, Future};
use std::mem::ManuallyDrop;
use std::ptr::NonNull;
use std::slice::from_raw_parts;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::task::Poll;

use atomic_waker::AtomicWaker;
use block2::RcBlock;
use crossbeam_queue::ArrayQueue;
use log::trace;
use objc2_core_foundation::{CFIndex, CFNumber, CFRetained};
use objc2_io_kit::{kIOHIDMaxInputReportSizeKey, kIOReturnSuccess, IOHIDDevice, IOHIDReportType, IOOptionBits, IOReturn};

use crate::backend::iohidmanager::device_info::property_key;
use crate::{ensure, AsyncHidFeatureHandle, AsyncHidRead, AsyncHidWrite, HidError, HidResult};

pub struct DeviceReadWriter {
    device: CFRetained<IOHIDDevice>,
    read_state: Option<ReaderState>,
    write_state: Option<WriterState>,
}

unsafe impl Send for DeviceReadWriter {}
unsafe impl Sync for DeviceReadWriter {}

struct ReaderState {
    inner: *const AsyncReportReaderInner,
    report_buffer: ManuallyDrop<Vec<u8>>,
}

struct WriterState;

unsafe impl Send for ReaderState {}
unsafe impl Sync for ReaderState {}

impl DeviceReadWriter {
    pub const DEVICE_OPTIONS: IOOptionBits = 0;

    pub fn new(device: CFRetained<IOHIDDevice>, read: bool, write: bool) -> HidResult<Self> {
        if read || write {
            ensure!(
                device.open(DeviceReadWriter::DEVICE_OPTIONS) == kIOReturnSuccess,
                HidError::message("Failed to open device")
            );
        }

        let read_state = match read {
            false => None,
            true => Some(unsafe {
                let max_input_report_len = device
                    .property(&property_key(kIOHIDMaxInputReportSizeKey))
                    .ok_or(HidError::message("Failed to read input report size"))?
                    .downcast_ref::<CFNumber>()
                    .and_then(|n| n.as_i32())
                    .unwrap() as usize;

                let mut report_buffer = ManuallyDrop::new(vec![0u8; max_input_report_len]);

                let inner = Box::into_raw(Box::new(AsyncReportReaderInner::default()));

                device.register_input_report_callback(
                    NonNull::new_unchecked(report_buffer.as_mut_ptr()),
                    report_buffer.len() as CFIndex,
                    Some(AsyncReportReaderInner::hid_report_callback),
                    inner.cast(),
                );
                device.register_removal_callback(Some(AsyncReportReaderInner::hid_removal_callback), inner.cast());

                ReaderState {
                    inner: inner.cast(),
                    report_buffer,
                }
            }),
        };

        let write_state = write.then_some(WriterState);

        device.activate();

        Ok(Self {
            device,
            read_state,
            write_state,
        })
    }

    /// Common function to write reports from the specified [`IOHIDReportType`]
    async fn write_report<'a>(&'a self, report_type: IOHIDReportType, buf: &'a [u8]) -> HidResult<()> {
        #[allow(non_upper_case_globals)]
        const kIOReturnBadArgument: IOReturn = objc2_io_kit::kIOReturnBadArgument as IOReturn;

        let _ = self.write_state.as_ref().expect("Device is not writable");
        let report_id = buf[0];
        let data_to_send = if report_id == 0x0 { &buf[1..] } else { buf };

        #[allow(non_upper_case_globals)]
        match unsafe {
            self.device.set_report(
                report_type,
                report_id as _,
                NonNull::new_unchecked(data_to_send.as_ptr() as _),
                data_to_send.len() as _,
            )
        } {
            kIOReturnSuccess => Ok(()),
            kIOReturnBadArgument => Err(HidError::Disconnected),
            other => Err(HidError::message(format!("failed to set report type: {:#X}", other))),
        }
    }
}

impl AsyncHidRead for Arc<DeviceReadWriter> {
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + Send + 'a {
        self.read_state
            .as_ref()
            .expect("Device is not readable")
            .read(buf)
    }
}

impl AsyncHidWrite for Arc<DeviceReadWriter> {
    async fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        self.write_report(IOHIDReportType::Output, buf).await
    }
}

impl AsyncHidFeatureHandle for Arc<DeviceReadWriter> {
    async fn read_feature_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
        #[allow(non_upper_case_globals)]
        const kIOReturnBadArgument: IOReturn = objc2_io_kit::kIOReturnBadArgument as IOReturn;

        #[allow(non_upper_case_globals)]
        const kIOReturnOverrun: IOReturn = objc2_io_kit::kIOReturnOverrun as IOReturn;

        let mut len: CFIndex = buf.len() as _;

        #[allow(non_upper_case_globals)]
        match unsafe {
            self.device.report(
                IOHIDReportType::Feature,
                buf[0] as _,
                NonNull::new_unchecked(buf.as_ptr() as _),
                NonNull::new_unchecked(&mut len),
            )
        } {
            kIOReturnSuccess => Ok(len as usize),
            kIOReturnBadArgument => Err(HidError::Disconnected),
            kIOReturnOverrun => Err(HidError::message("read feature report overrun")),
            other => Err(HidError::message(format!("failed to read feature report: {:#X}", other))),
        }
    }

    async fn write_feature_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
        self.write_report(IOHIDReportType::Feature, buf).await
    }
}

impl ReaderState {
    pub fn read<'a>(&'a self, buf: &'a mut [u8]) -> impl Future<Output = HidResult<usize>> + 'a {
        poll_fn(|cx| {
            let inner = unsafe { &*self.inner };
            inner.waker.register(cx.waker());
            match inner.full_buffers.pop() {
                Some(report) => {
                    let length = report.len().min(buf.len());
                    buf[..length].copy_from_slice(&report[..length]);
                    inner.recycle_buffer(report);
                    Poll::Ready(Ok(length))
                }
                None => match inner.removed.load(Ordering::Relaxed) {
                    true => Poll::Ready(Err(HidError::Disconnected)),
                    false => Poll::Pending,
                },
            }
        })
    }
}

impl Drop for DeviceReadWriter {
    fn drop(&mut self) {
        unsafe {
            {
                let once = Arc::new(Once::new());
                let block = RcBlock::new({
                    let once = once.clone();
                    move || once.call_once(|| trace!("Finished canceling device"))
                });

                self.device.set_cancel_handler(RcBlock::as_ptr(&block));
                self.device.cancel();
                trace!("Waiting for device cancel to finish");
                once.wait();
                trace!("Resuming destructor of device");
            }

            if let Some(mut state) = self.read_state.take() {
                //SAFETY The device was canceled in the previous step,
                // and therefore the callbacks that reference these buffers can no longer be called
                ManuallyDrop::drop(&mut state.report_buffer);
                drop(Box::<AsyncReportReaderInner>::from_raw(state.inner as *mut _));
            }

            self.device.close(Self::DEVICE_OPTIONS);
        }
    }
}

struct AsyncReportReaderInner {
    full_buffers: ArrayQueue<Vec<u8>>,
    empty_buffers: ArrayQueue<Vec<u8>>,
    removed: AtomicBool,
    waker: AtomicWaker,
}

impl Default for AsyncReportReaderInner {
    fn default() -> Self {
        Self {
            full_buffers: ArrayQueue::new(64),
            empty_buffers: ArrayQueue::new(8),
            removed: AtomicBool::new(false),
            waker: AtomicWaker::new(),
        }
    }
}

impl AsyncReportReaderInner {
    fn recycle_buffer(&self, buf: Vec<u8>) {
        let _ = self.empty_buffers.push(buf);
    }

    unsafe extern "C-unwind" fn hid_report_callback(
        context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _report_type: IOHIDReportType, _report_id: u32, report: NonNull<u8>,
        report_length: CFIndex,
    ) {
        let this: &Self = &*(context as *mut Self);
        let mut buffer = this.empty_buffers.pop().unwrap_or_default();
        buffer.resize(report_length as usize, 0);
        buffer.copy_from_slice(from_raw_parts(report.as_ptr(), report_length as usize));
        if let Some(old) = this.full_buffers.force_push(buffer) {
            this.recycle_buffer(old);
        }
        this.waker.wake();
    }

    unsafe extern "C-unwind" fn hid_removal_callback(context: *mut c_void, _result: IOReturn, _sender: *mut c_void) {
        let this: &Self = &*(context as *mut Self);
        this.removed.store(true, Ordering::Relaxed);
        this.waker.wake();
    }
}
