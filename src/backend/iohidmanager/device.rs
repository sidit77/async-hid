use std::ffi::c_void;
use std::mem::{ManuallyDrop};
use std::ptr::null_mut;
use std::slice::from_raw_parts;

use core_foundation::base::{kCFAllocatorDefault, CFIndex, CFRelease, CFType, TCFType};
use core_foundation::number::CFNumber;
use core_foundation::runloop::CFRunLoop;
use core_foundation::string::CFString;
use core_foundation::{impl_TCFType, ConcreteCFType};
use io_kit_sys::hid::base::{IOHIDDeviceRef, IOHIDReportCallback};
use io_kit_sys::hid::device::{IOHIDDeviceClose, IOHIDDeviceCreate, IOHIDDeviceGetProperty, IOHIDDeviceGetTypeID, IOHIDDeviceOpen, IOHIDDeviceScheduleWithRunLoop, IOHIDDeviceSetReport, IOHIDDeviceUnscheduleFromRunLoop};
use io_kit_sys::hid::keys::{kIOHIDMaxInputReportSizeKey, IOHIDReportType};
use io_kit_sys::ret::{kIOReturnSuccess, IOReturn};
use io_kit_sys::types::IOOptionBits;

use crate::backend::iohidmanager::service::{IOService, RegistryEntryId};
use crate::backend::iohidmanager::utils::Key;
use crate::{ensure, HidError, HidResult};

extern "C" {
    // Workaround for https://github.com/jtakakura/io-kit-rs/issues/6
    fn IOHIDDeviceRegisterInputReportCallback(device: IOHIDDeviceRef, report: *mut u8, report_length: CFIndex, callback: Option<IOHIDReportCallback>, context: *mut c_void);
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
        ensure!(!value.is_null(), HidError::custom("IOHIDDevice is null"));
        Ok(unsafe { IOHIDDevice::wrap_under_get_rule(value) })
    }
}

impl TryFrom<RegistryEntryId> for IOHIDDevice {
    type Error = HidError;

    fn try_from(value: RegistryEntryId) -> Result<Self, Self::Error> {
        unsafe {
            let service = IOService::try_from(value)?;
            let device = IOHIDDeviceCreate(kCFAllocatorDefault, service.raw());
            ensure!(!device.is_null(), HidError::custom(format!("Failed to open device at port {:?}", value)));
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
        ensure!(!property_ref.is_null(), HidError::custom("Failed to retrieve property"));
        let property = unsafe { CFType::wrap_under_get_rule(property_ref) };
        Ok(property)
    }

    pub fn property<T: ConcreteCFType>(&self, key: impl Key) -> HidResult<T> {
        self.untyped_property(key)?
            .downcast_into::<T>()
            .ok_or(HidError::custom("Failed to cast property"))
    }

    pub fn get_i32_property(&self, key: impl Key) -> HidResult<i32> {
        self.property::<CFNumber>(key)
            .and_then(|v| v.to_i32().ok_or(HidError::custom("Property is not an i32")))
    }

    pub fn get_string_property(&self, key: impl Key) -> HidResult<String> {
        self.property::<CFString>(key).map(|v| v.to_string())
    }

    pub fn open(&self, options: IOOptionBits) -> HidResult<()> {
        let ret = unsafe { IOHIDDeviceOpen(self.as_concrete_TypeRef(), options) };
        //TODO check for kIOReturnNotPermitted
        ensure!(
            ret == kIOReturnSuccess,
            HidError::custom(format!("failed to open IOHIDDevice: {:?}", ret))
        );
        Ok(())
    }

    pub fn close(&self, options: IOOptionBits) -> HidResult<()> {
        let ret = unsafe { IOHIDDeviceClose(self.as_concrete_TypeRef(), options) };
        ensure!(
            ret == kIOReturnSuccess,
            HidError::custom(format!("failed to close IOHIDDevice: {:?}", ret))
        );
        Ok(())
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
        let ret = unsafe { IOHIDDeviceSetReport(self.as_concrete_TypeRef(), report_type, report_id, report.as_ptr(), report.len() as _) };
        ensure!(ret == kIOReturnSuccess, HidError::custom(format!("Failed to send report: {}", ret)));
        Ok(())
    }

    pub fn register_input_report_callback<F>(&self, callback: F) -> HidResult<CallbackGuard>
        where
            F: FnMut(&[u8]) + Send + Sync + 'static
    {
        let max_input_report_len = self.get_i32_property(kIOHIDMaxInputReportSizeKey)? as usize;

        let mut report_buffer = ManuallyDrop::new(vec![0u8; max_input_report_len]);

        let (report_buffer_ptr, report_buffer_len, report_buffer_capacity) = (report_buffer.as_mut_ptr(), report_buffer.len(), report_buffer.capacity());

        let callback: InputReportCallback = Box::new(callback);
        let callback: InputReportCallbackContainer = Box::new(callback);

        let callback_ptr = Box::into_raw(callback);

        unsafe {
            IOHIDDeviceRegisterInputReportCallback(
                self.as_concrete_TypeRef(),
                report_buffer_ptr as _,
                report_buffer_len as _,
                Some(hid_report_callback),
                callback_ptr as _,
            );
        }

        Ok(CallbackGuard {
            device: self.clone(),
            report_buffer_ptr,
            report_buffer_len,
            report_buffer_capacity,
            callback_ptr,
        })
    }
}

type InputReportCallback = Box<dyn FnMut(&[u8]) + Send + Sync>;
type InputReportCallbackContainer = Box<InputReportCallback>;

#[must_use = "The callback will be unregistered when the returned guard is dropped"]
pub struct CallbackGuard {
    device: IOHIDDevice,
    report_buffer_ptr: *mut u8,
    report_buffer_len: usize,
    report_buffer_capacity: usize,
    callback_ptr: *mut InputReportCallback,
}

unsafe impl Send for CallbackGuard {}
unsafe impl Sync for CallbackGuard {}

impl Drop for CallbackGuard {
    fn drop(&mut self) {
        unsafe {
            IOHIDDeviceRegisterInputReportCallback(self.device.as_concrete_TypeRef(), null_mut(), 0, None, null_mut())
        }

        drop(unsafe { Vec::<u8>::from_raw_parts(self.report_buffer_ptr, self.report_buffer_len, self.report_buffer_capacity) });

        drop(unsafe { InputReportCallbackContainer::from_raw(self.callback_ptr) });
    }
}

unsafe extern "C" fn hid_report_callback(
    context: *mut c_void, _result: IOReturn, _sender: *mut c_void, _report_type: IOHIDReportType, _report_id: u32, report: *mut u8,
    report_length: CFIndex
) {
    let callback: &mut InputReportCallback = &mut *(context as *mut InputReportCallback);
    let data = from_raw_parts(report, report_length as usize);
    callback(data);
}