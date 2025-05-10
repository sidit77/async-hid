use crate::backend::iohidmanager2::device_info::property_key;
use crate::{AsyncHidRead, AsyncHidWrite, HidError, HidResult};
use atomic_waker::AtomicWaker;
use crossbeam_queue::ArrayQueue;
use objc2_core_foundation::{CFIndex, CFNumber, CFRetained};
use objc2_io_kit::{kIOHIDMaxInputReportSizeKey, IOHIDDevice, IOHIDReportType, IOOptionBits, IOReturn};
use std::ffi::c_void;
use std::future::{poll_fn, Future};
use std::mem::ManuallyDrop;
use std::ptr::{null_mut, NonNull};
use std::slice::from_raw_parts;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

pub struct DeviceReadWriter {
    device: CFRetained<IOHIDDevice>,
    read_state: Option<ReaderState>,
}

unsafe impl Send for DeviceReadWriter {}
unsafe impl Sync for DeviceReadWriter {}

pub struct ReaderState {
    inner: *const AsyncReportReaderInner,
    report_buffer: ManuallyDrop<Vec<u8>>,
}

unsafe impl Send for ReaderState {}
unsafe impl Sync for ReaderState {}

impl DeviceReadWriter {
    
    pub const DEVICE_OPTIONS: IOOptionBits = 0;
    
    pub fn new(device: CFRetained<IOHIDDevice>, read: bool) -> HidResult<Self> {
        
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
                    inner.cast()
                );
                device.register_removal_callback(
                    Some(AsyncReportReaderInner::hid_removal_callback), 
                    inner.cast()
                );
                
                ReaderState {
                    inner: inner.cast(),
                    report_buffer,
                }
            })
        };
        
        unsafe { device.activate(); }
        Ok(Self { device, read_state })
    }
    
    pub fn reader(&self) -> &ReaderState {
        self.read_state.as_ref().expect("Device is not readable")
    }
    
}

impl AsyncHidRead for Arc<DeviceReadWriter> {
    fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> impl Future<Output=HidResult<usize>> + Send + 'a {
        self
            .read_state
            .as_ref()
            .expect("Device was not initialized for read operations")
            .read(buf)
    }
}

impl AsyncHidWrite for Arc<DeviceReadWriter> {
    async fn write_output_report<'a>(&'a mut self, _buf: &'a [u8]) -> HidResult<()> {
        todo!()
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
                None => match inner.removed.load(Ordering::SeqCst) {
                    true => Poll::Ready(Err(HidError::Disconnected)),
                    false => Poll::Pending,
                }
            }
        })
    }
}

impl Drop for DeviceReadWriter {
    fn drop(&mut self) {
        unsafe {
            self.device.cancel();
            
            if let Some(mut state) = self.read_state.take() {
                self.device.register_removal_callback(None, null_mut());
                self.device.register_input_report_callback(NonNull::dangling(), 0, None, null_mut());

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
        context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _report_type: IOHIDReportType, _report_id: u32, report: NonNull<u8>, report_length: CFIndex
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
        this.removed.store(true, Ordering::SeqCst);
        this.waker.wake();
    }

}