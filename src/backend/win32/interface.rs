use std::ffi::c_void;
use std::pin::Pin;
use std::ptr::null;
use std::sync::OnceLock;
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use crossbeam_queue::ArrayQueue;
use futures_lite::Stream;
use log::debug;
use windows::core::{Owned, GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW, CM_Register_Notification, CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
    CM_NOTIFY_ACTION, CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL, CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL, CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER,
    CM_NOTIFY_FILTER_0, CM_NOTIFY_FILTER_0_0, CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE, CR_BUFFER_SMALL, CR_SUCCESS, HCMNOTIFICATION
};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
use windows::Win32::Foundation::ERROR_SUCCESS;

use crate::backend::win32::string::U16StringList;
use crate::{DeviceEvent, DeviceId, HidResult};

pub struct Interface;

impl Interface {
    /*
        fn get_property_size<T: DeviceProperty>(
            interface: &U16Str,
            property_key: impl PropertyKey,
        ) -> WinResult<usize> {
            let mut property_type = 0;
            let mut len = 0;
            let cr = unsafe {
                CM_Get_Device_Interface_PropertyW(
                    interface.as_ptr(),
                    property_key.as_ptr(),
                    &mut property_type,
                    null_mut(),
                    &mut len,
                    0,
                )
            };
            check_config(cr, CR_BUFFER_SMALL)?;
            ensure!(
                property_type == T::TYPE,
                Err(WinError::WrongPropertyDataType)
            );
            Ok(len as usize)
        }



        pub fn get_property<T: DeviceProperty>(interface: &U16Str, property_key: impl PropertyKey) -> WinResult<T> {
            let size = Self::get_property_size::<T>(interface, property_key)?;
            let mut property = T::create_sized(size);
            let mut property_type = 0;
            let mut len = size as u32;
            let cr = unsafe {
                CM_Get_Device_Interface_PropertyW(
                    interface.as_ptr(),
                    property_key.as_ptr(),
                    &mut property_type,
                    property.as_ptr_mut(),
                    &mut len,
                    0,
                )
            };
            check_config(cr, CR_SUCCESS)?;
            ensure!(size == len as usize, Err(WinError::UnexpectedReturnSize));
            property.validate();
            Ok(property)
        }

    */

    fn guid() -> &'static GUID {
        static CACHE: OnceLock<GUID> = OnceLock::new();
        CACHE.get_or_init(|| unsafe { HidD_GetHidGuid() })
    }

    fn get_interface_list_length(interface: GUID) -> HidResult<usize> {
        let mut len = 0;
        match unsafe { CM_Get_Device_Interface_List_SizeW(&mut len, &interface, None, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) } {
            CR_SUCCESS => Ok(len as usize),
            err => Err(err.into())
        }
    }

    pub fn get_interface_list() -> HidResult<U16StringList> {
        let mut device_interface_list = vec![0; Self::get_interface_list_length(*Self::guid())?];
        loop {
            match unsafe {
                CM_Get_Device_Interface_ListW(
                    Self::guid(),
                    None,
                    device_interface_list.as_mut_slice(),
                    CM_GET_DEVICE_INTERFACE_LIST_PRESENT
                )
            } {
                CR_SUCCESS => return Ok(unsafe { U16StringList::from_vec_unchecked(device_interface_list) }),
                CR_BUFFER_SMALL => device_interface_list.resize(Self::get_interface_list_length(*Self::guid())?, 0),
                err => return Err(err.into())
            }
        }
    }
}

pub struct DeviceNotificationStream {
    registration: Option<Owned<HCMNOTIFICATION>>,
    inner: *const DeviceNotificationStreamInner
}

struct DeviceNotificationStreamInner {
    queue: ArrayQueue<DeviceEvent>,
    waker: AtomicWaker
}

unsafe impl Send for DeviceNotificationStream {}

impl DeviceNotificationStream {
    pub fn new() -> HidResult<Self> {
        let filter = CM_NOTIFY_FILTER {
            cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
            Flags: 0,
            FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
            Reserved: 0,
            u: CM_NOTIFY_FILTER_0 {
                DeviceInterface: CM_NOTIFY_FILTER_0_0 {
                    ClassGuid: *Interface::guid()
                }
            }
        };
        let inner = Box::into_raw(Box::new(DeviceNotificationStreamInner {
            queue: ArrayQueue::new(64),
            waker: AtomicWaker::new()
        }));
        let mut handle = HCMNOTIFICATION::default();
        match unsafe { CM_Register_Notification(&filter, Some(inner as *const c_void), Some(Self::callback), &mut handle) } {
            CR_SUCCESS => Ok(Self {
                registration: Some(unsafe { Owned::new(handle) }),
                inner
            }),
            err => {
                drop(unsafe { Box::from_raw(inner) });
                Err(err.into())
            }
        }
    }

    unsafe extern "system" fn callback(
        _: HCMNOTIFICATION, context: *const c_void, action: CM_NOTIFY_ACTION, eventdata: *const CM_NOTIFY_EVENT_DATA, _: u32
    ) -> u32 {
        if !matches!(action, CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL | CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL) {
            return ERROR_SUCCESS.0;
        }
        let data = unsafe { &*eventdata };
        assert_eq!(data.FilterType, CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE);
        let data = unsafe { &data.u.DeviceInterface };
        assert_eq!(data.ClassGuid, *Interface::guid());
        let device_id = unsafe { PCWSTR::from_raw(data.SymbolicLink.as_ptr()).to_hstring() };
        let event = match action {
            CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL => Some(DeviceEvent::Connected(DeviceId::UncPath(device_id))),
            CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL => Some(DeviceEvent::Disconnected(DeviceId::UncPath(device_id))),
            _ => {
                debug!("Unknown device event: {}", action.0);
                None
            }
        };
        if let Some(event) = event {
            let inner = unsafe { &*(context as *const DeviceNotificationStreamInner) };
            inner.queue.force_push(event);
            inner.waker.wake();
        }

        ERROR_SUCCESS.0
    }
}

impl Drop for DeviceNotificationStream {
    fn drop(&mut self) {
        drop(self.registration.take());
        drop(unsafe { Box::from_raw(self.inner as *mut DeviceNotificationStreamInner) });
        self.inner = null();
    }
}

impl Stream for DeviceNotificationStream {
    type Item = DeviceEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let inner = unsafe { &*(self.inner) };
        inner.waker.register(cx.waker());
        match inner.queue.pop() {
            None => Poll::Pending,
            Some(e) => Poll::Ready(Some(e))
        }
    }
}
