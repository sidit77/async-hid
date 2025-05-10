use std::ffi::CStr;
use std::mem::transmute;
use objc2_core_foundation::{kCFAllocatorNull, CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFStringBuiltInEncodings};
use objc2_io_kit::{kIOHIDDeviceUsageKey, kIOHIDDeviceUsagePageKey, kIOHIDDeviceUsagePairsKey, kIOHIDPrimaryUsageKey, kIOHIDPrimaryUsagePageKey, kIOHIDProductIDKey, kIOHIDProductKey, kIOHIDSerialNumberKey, kIOHIDVendorIDKey, kIOReturnSuccess, IOHIDDevice, IORegistryEntryGetRegistryEntryID};
use crate::{ensure, DeviceId, DeviceInfo, HidError, HidResult};

pub fn get_device_info(device: &IOHIDDevice) -> HidResult<Vec<DeviceInfo>> {
    let name = unsafe {
        device
            .property(&property_key(kIOHIDProductKey))
            .ok_or(HidError::message("Failed to get the name"))?
            .downcast_ref::<CFString>()
            .unwrap()
            .to_string()
    };

    let product_id = unsafe {
        device
            .property(&property_key(kIOHIDProductIDKey))
            .ok_or(HidError::message("Failed to get the product id"))?
            .downcast_ref::<CFNumber>()
            .and_then(CFNumber::as_i32)
            .unwrap() as u16
    };

    let vendor_id = unsafe {
        device
            .property(&property_key(kIOHIDVendorIDKey))
            .ok_or(HidError::message("Failed to get the vendor id"))?
            .downcast_ref::<CFNumber>()
            .and_then(CFNumber::as_i32)
            .unwrap() as u16
    };

    let primary_usage_page = unsafe {
        device
            .property(&property_key(kIOHIDPrimaryUsagePageKey))
            .ok_or(HidError::message("Failed to get the primary usage page"))?
            .downcast_ref::<CFNumber>()
            .and_then(CFNumber::as_i32)
            .unwrap() as u16
    };

    let primary_usage_id = unsafe {
        device
            .property(&property_key(kIOHIDPrimaryUsageKey))
            .ok_or(HidError::message("Failed to get the primary usage id"))?
            .downcast_ref::<CFNumber>()
            .and_then(CFNumber::as_i32)
            .unwrap() as u16
    };

    let serial_number = unsafe {
        device
            .property(&property_key(kIOHIDSerialNumberKey))
            .and_then(|p| p
                .downcast_ref::<CFString>()
                .map(|p| p.to_string()))
    };

    

    let primary_info = DeviceInfo {
        id: get_device_id(device)?,
        name,
        product_id,
        vendor_id,
        usage_id: primary_usage_id,
        usage_page: primary_usage_page,
        serial_number,
    };

    let mut result = vec![primary_info.clone()];
    result.extend(unsafe {
        device
            .property(&property_key(kIOHIDDeviceUsagePairsKey))
            .iter()
            .flat_map(|p | p
                .downcast_ref::<CFArray>()
                .map(|p| transmute::<&CFArray, &CFArray<CFDictionary<CFString, CFNumber>>>(p))
                .unwrap()
                .iter()
                .map(|dict|  {
                    let usage = dict
                        .get(&property_key(kIOHIDDeviceUsageKey))
                        .and_then(|p| p.as_i32())
                        .unwrap() as u16;
                    let usage_page = dict
                        .get(&property_key(kIOHIDDeviceUsagePageKey))
                        .and_then(|p| p.as_i32())
                        .unwrap() as u16;
                    (usage, usage_page)
                }))
            .filter(|(usage, usage_page)| (*usage_page != primary_usage_page) || (*usage != primary_usage_id))
            .map(move |(usage, usage_page)| DeviceInfo {
                usage_id: usage,
                usage_page: usage_page,
                ..primary_info.clone()
            })
    });

    Ok(result)
}

pub fn property_key(key: &'static CStr) -> CFRetained<CFString> {
    unsafe { CFString::with_c_string_no_copy(None, key.as_ptr(), CFStringBuiltInEncodings::EncodingUTF8.0, kCFAllocatorNull).unwrap() }
}

pub fn get_device_id(device: &IOHIDDevice) -> HidResult<DeviceId> {
    let mut id = 0;
    let port = unsafe { device.service() };
    ensure!(port != 0, HidError::message("Failed to get mach port"));
    ensure!(unsafe { IORegistryEntryGetRegistryEntryID(port, &mut id) } == kIOReturnSuccess, 
        HidError::message("Failed to retrieve entry id"));
    Ok(DeviceId::RegistryEntryId(id))
}