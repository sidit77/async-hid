use windows::core::GUID;
use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW, CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CR_BUFFER_SMALL, CR_SUCCESS};
use windows::Win32::Devices::HumanInterfaceDevice::HidD_GetHidGuid;
use crate::backend::win32::string::U16StringList;
use crate::HidResult;

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
    fn get_interface_list_length(interface: GUID) -> HidResult<usize> {
        let mut len = 0;
        match unsafe { CM_Get_Device_Interface_List_SizeW(&mut len, &interface, None, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) } {
            CR_SUCCESS => Ok(len as usize),
            err => Err(err.into()),
        }
    }

    pub fn get_interface_list() -> HidResult<U16StringList> {
        let iface = unsafe { HidD_GetHidGuid() };

        let mut device_interface_list = Vec::new();
        loop {
            device_interface_list.resize(Self::get_interface_list_length(iface)?, 0);
            match unsafe { CM_Get_Device_Interface_ListW(&iface, None, device_interface_list.as_mut_slice(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT) } {
                CR_SUCCESS => return Ok(unsafe { U16StringList::from_vec_unchecked(device_interface_list) }),
                CR_BUFFER_SMALL => continue,
                err => return Err(err.into()),
            }
        }
    }

}
