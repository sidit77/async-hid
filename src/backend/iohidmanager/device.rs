use atomic_waker::AtomicWaker;
use core_foundation::base::{kCFAllocatorDefault, CFIndex, CFRelease, CFType, TCFType};
use core_foundation::number::CFNumber;
use core_foundation::runloop::CFRunLoop;
use core_foundation::string::CFString;
use core_foundation::{impl_TCFType, ConcreteCFType};
use crossbeam_queue::ArrayQueue;
use io_kit_sys::hid::base::{IOHIDCallback, IOHIDDeviceRef, IOHIDReportCallback};
use io_kit_sys::hid::device::{IOHIDDeviceClose, IOHIDDeviceCreate, IOHIDDeviceGetProperty, IOHIDDeviceGetTypeID, IOHIDDeviceOpen, IOHIDDeviceScheduleWithRunLoop, IOHIDDeviceSetReport, IOHIDDeviceUnscheduleFromRunLoop};
use io_kit_sys::hid::keys::{kIOHIDMaxInputReportSizeKey, IOHIDReportType};
use io_kit_sys::ret::{kIOReturnBadArgument, kIOReturnSuccess, IOReturn};
use io_kit_sys::types::IOOptionBits;
use std::ffi::c_void;
use std::future::{poll_fn, Future};
use std::mem::ManuallyDrop;
use std::ptr::null_mut;
use std::slice::from_raw_parts;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

use crate::backend::iohidmanager::service::{IOService, RegistryEntryId};
use crate::backend::iohidmanager::utils::Key;
use crate::{ensure, HidError, HidResult};

extern "C" {
    // Workaround for https://github.com/jtakakura/io-kit-rs/issues/6
    fn IOHIDDeviceRegisterInputReportCallback(device: IOHIDDeviceRef, report: *mut u8, report_length: CFIndex, callback: Option<IOHIDReportCallback>, context: *mut c_void);
    fn IOHIDDeviceRegisterRemovalCallback(device: IOHIDDeviceRef, callback: Option<IOHIDCallback>, context: *mut c_void);
}

#[derive(Debug)]
#[repr(transparent)]
pub struct IOHIDDevice(IOHIDDeviceRef);

impl_TCFType!(IOHIDDevice, IOHIDDeviceRef, IOHIDDeviceGetTypeID);
unsafe impl Send for IOHIDDevice {}
unsafe impl Sync for IOHIDDevice {}

impl TryFrom<IOHIDDeviceRef> for IOHIDDevice {
    type Error = HidError;

    fn try_from(value: IOHIDDeviceRef) -> Result<Self, Self::Error> {
        ensure!(!value.is_null(), HidError::message("IOHIDDevice is null"));
        Ok(unsafe { IOHIDDevice::wrap_under_get_rule(value) })
    }
}

impl TryFrom<RegistryEntryId> for IOHIDDevice {
    type Error = HidError;

    fn try_from(value: RegistryEntryId) -> Result<Self, Self::Error> {
        unsafe {
            let service = IOService::try_from(value)
                .map_err(|_| HidError::NotConnected)?;
            let device = IOHIDDeviceCreate(kCFAllocatorDefault, service.raw());
            ensure!(!device.is_null(), HidError::message(format!("Failed to open device at port {:?}", value)));
            Ok(IOHIDDevice::wrap_under_create_rule(device))
        }
    }
}

impl Drop for IOHIDDevice {
    fn drop(&mut self) {
        unsafe { CFRelease(self.as_CFTypeRef()) }
    }
}

impl IOHIDDevice {
    pub fn untyped_property(&self, key: impl Key) -> HidResult<CFType> {
        let key = key.to_string();
        let property_ref = unsafe { IOHIDDeviceGetProperty(self.as_concrete_TypeRef(), key.as_concrete_TypeRef()) };
        ensure!(!property_ref.is_null(), HidError::message("Failed to retrieve property"));
        let property = unsafe { CFType::wrap_under_get_rule(property_ref) };
        Ok(property)
    }

    pub fn property<T: ConcreteCFType>(&self, key: impl Key) -> HidResult<T> {
        self.untyped_property(key)?
            .downcast_into::<T>()
            .ok_or(HidError::message("Failed to cast property"))
    }

    pub fn get_i32_property(&self, key: impl Key) -> HidResult<i32> {
        self.property::<CFNumber>(key)
            .and_then(|v| v.to_i32().ok_or(HidError::message("Property is not an i32")))
    }

    pub fn get_string_property(&self, key: impl Key) -> HidResult<String> {
        self.property::<CFString>(key).map(|v| v.to_string())
    }

    pub fn open(&self, options: IOOptionBits) -> HidResult<()> {
        let ret = unsafe { IOHIDDeviceOpen(self.as_concrete_TypeRef(), options) };
        //TODO check for kIOReturnNotPermitted
        ensure!(
            ret == kIOReturnSuccess,
            HidError::message(format!("failed to open IOHIDDevice: {:#X}", ret))
        );
        Ok(())
    }

    pub fn close(&self, options: IOOptionBits) -> HidResult<()> {
        #[allow(non_upper_case_globals)]
        match unsafe { IOHIDDeviceClose(self.as_concrete_TypeRef(), options) } {
            kIOReturnSuccess => Ok(()),
            kIOReturnBadArgument => Err(HidError::Disconnected),
            other => Err(HidError::message(format!("failed to close IOHIDDevice: {:#X}", other))),
        }
    }

    pub fn schedule_with_runloop(&self, runloop: &CFRunLoop, mode: &CFString) {
        unsafe {
            IOHIDDeviceScheduleWithRunLoop(self.as_concrete_TypeRef(), runloop.as_concrete_TypeRef(), mode.as_concrete_TypeRef());
        }
    }

    pub fn unschedule_from_runloop(&self, runloop: &CFRunLoop, mode: &CFString) {
        unsafe {
            IOHIDDeviceUnscheduleFromRunLoop(self.as_concrete_TypeRef(), runloop.as_concrete_TypeRef(), mode.as_concrete_TypeRef());
        }
    }

    pub fn set_report(&self, report_type: IOHIDReportType, report_id: CFIndex, report: &[u8]) -> HidResult<()> {
        //TODO make this async using IOHIDDeviceSetReportWithCallback
        #[allow(non_upper_case_globals)]
        match unsafe { IOHIDDeviceSetReport(self.as_concrete_TypeRef(), report_type, report_id, report.as_ptr(), report.len() as _) } {
            kIOReturnSuccess => Ok(()),
            kIOReturnBadArgument => Err(HidError::Disconnected),
            other => Err(HidError::message(format!("failed to set report type: {:#X}", other))),
        }
    }

}


pub struct AsyncReportReader {
    inner: *const AsyncReportReaderInner,
    device: IOHIDDevice,
    report_buffer: ManuallyDrop<Vec<u8>>,
}

impl AsyncReportReader {

    pub fn new(device: &IOHIDDevice) -> HidResult<AsyncReportReader> {
        let max_input_report_len = device.get_i32_property(kIOHIDMaxInputReportSizeKey)? as usize;

        let mut report_buffer = ManuallyDrop::new(vec![0u8; max_input_report_len]);

        let inner = Box::into_raw(Box::new(AsyncReportReaderInner::default()));

        unsafe {
            IOHIDDeviceRegisterRemovalCallback(device.as_concrete_TypeRef(), Some(AsyncReportReaderInner::hid_removal_callback), inner as _);
            IOHIDDeviceRegisterInputReportCallback(
                device.as_concrete_TypeRef(),
                report_buffer.as_mut_ptr() as _,
                report_buffer.len() as _,
                Some(AsyncReportReaderInner::hid_report_callback),
                inner as _,
            );
        }

        Ok(AsyncReportReader {
            device: device.clone(),
            report_buffer,
            inner,
        })
    }

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
                }
            }
        })
    }

}

unsafe impl Send for AsyncReportReader {}
unsafe impl Sync for AsyncReportReader {}

impl Drop for AsyncReportReader {
    fn drop(&mut self) {
        unsafe {
            IOHIDDeviceRegisterRemovalCallback(self.device.as_concrete_TypeRef(), None, null_mut());
            IOHIDDeviceRegisterInputReportCallback(self.device.as_concrete_TypeRef(), null_mut(), 0, None, null_mut())
        }

        unsafe { ManuallyDrop::drop(&mut self.report_buffer) };
        unsafe { drop(Box::<AsyncReportReaderInner>::from_raw(self.inner as *mut _)) };
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

    unsafe extern "C" fn hid_report_callback(
        context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _report_type: IOHIDReportType, _report_id: u32, report: *mut u8,
        report_length: CFIndex
    ) {
        let this: &Self = &*(context as *mut Self);
        let mut buffer = this.empty_buffers.pop().unwrap_or(Vec::new());
        buffer.resize(report_length as usize, 0);
        buffer.copy_from_slice(from_raw_parts(report, report_length as usize));
        if let Some(old) = this.full_buffers.force_push(buffer) {
            this.recycle_buffer(old);
        }
        this.waker.wake();
    }

    unsafe extern "C" fn hid_removal_callback(context: *mut c_void, _result: IOReturn, _sender: *mut c_void) {
        let this: &Self = &*(context as *mut Self);
        this.removed.store(true, Ordering::Relaxed);
        this.waker.wake();
    }

}