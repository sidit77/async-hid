use crate::{DeviceInfo, HidResult};
use std::fmt::{Debug};
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use futures_core::Stream;
use futures_core::stream::BoxStream;
use crate::traits::{AsyncHidRead, AsyncHidWrite};
use crate::device_info::DeviceId;

//#[cfg(all(target_os = "windows", feature = "win32"))]
//mod win32;

//#[cfg(all(target_os = "windows", feature = "winrt"))]
//mod winrt;

#[cfg(target_os = "linux")]
mod hidraw;

#[cfg(target_os = "macos")]
mod iohidmanager;
#[cfg(target_os = "macos")]
pub use iohidmanager::{enumerate, open, BackendDevice, BackendDeviceId, BackendError, BackendPrivateData};


pub type DeviceInfoStream = BoxStream<'static, DeviceInfo>;
pub trait Backend: Sized + Default {
    type Reader: AsyncHidRead + Send + Sync;
    type Writer: AsyncHidWrite + Send + Sync;

    fn enumerate(&self) -> impl Future<Output = HidResult<DeviceInfoStream>> + Send;

    fn open(&self, id: &DeviceId, read: bool, write: bool) -> impl Future<Output = HidResult<(Option<Self::Reader>, Option<Self::Writer>)>> + Send;

}

macro_rules! dyn_backend_impl {
    {
        $(
            $(#[$module_attrs:meta])*
            mod $module:ident {
                $(#[$item_attrs:meta])*
                $name:ident($backend:ty)
            }
        )+
    } => {
        $(
            $(#[$module_attrs])*
            mod $module;
        )+
        
        #[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
        #[non_exhaustive]
        pub enum BackendType {
            $(
                $(#[$module_attrs])*$(#[$item_attrs])*
                $name,
            )+
        }
        
        enum DynReader {
            $(
                $(#[$module_attrs])*$(#[$item_attrs])*
                $name(<$backend as Backend>::Reader),
            )+
        }
        impl AsyncHidRead for DynReader {
            async fn read_input_report<'a>(&'a mut self, buf: &'a mut [u8]) -> HidResult<usize> {
                match self {
                    $(
                        $(#[$module_attrs])*$(#[$item_attrs])*
                        Self::$name(i) => i.read_input_report(buf).await,
                    )+
                }
            }
        }
        
        enum DynWriter {
            $(
                $(#[$module_attrs])*$(#[$item_attrs])*
                $name(<$backend as Backend>::Writer),
            )+
        }
        impl AsyncHidWrite for DynWriter {
            async fn write_output_report<'a>(&'a mut self, buf: &'a [u8]) -> HidResult<()> {
                match self {
                    $(
                        $(#[$module_attrs])*$(#[$item_attrs])*
                        Self::$name(i) => i.write_output_report(buf).await,
                    )+
                }
            }
        }
        
         pub enum DynBackend {
            $(
                $(#[$module_attrs])*$(#[$item_attrs])*
                $name($backend),
            )+
        }
        impl DynBackend {
            pub fn new(backend: BackendType) -> DynBackend {
                match backend {
                    $(
                        $(#[$module_attrs])*$(#[$item_attrs])*
                        BackendType::$name => Self::$name(<$backend as Default>::default()),
                    )+
                }
            }
        }
        impl Backend for DynBackend {
            type Reader = DynReader;
            type Writer = DynWriter;
        
            
            async fn enumerate(&self) -> HidResult<DeviceInfoStream> {
                match self {
                    $(
                        $(#[$module_attrs])*$(#[$item_attrs])*
                        Self::$name(i) => i.enumerate().await,
                    )+
                }
            }
        
            async fn open(&self, id: &DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
                match self {
                    $(
                        $(#[$module_attrs])*$(#[$item_attrs])*
                        Self::$name(i) => i.open(id, read, write).await.map(|(r, w)| (r.map(DynReader::$name), w.map(DynWriter::$name))),
                    )+
                }
            }
        }
    };
}

dyn_backend_impl! {
    #[cfg(all(target_os = "windows", feature = "win32"))]
    mod win32 {
        Win32(win32::Win32Backend)
    }
    #[cfg(all(target_os = "windows", feature = "winrt"))]
    mod winrt {
        WinRt(winrt::WinRtBackend)
    }
}

impl Default for DynBackend {
    fn default() -> Self {
        if cfg!(target_os = "windows") {
            if cfg!(feature = "win32") {
                return Self::new(BackendType::Win32);
            } else if cfg!(feature = "winrt") {
                return Self::new(BackendType::WinRt);
            }
        }
        panic!("No suitable backend found");
    }
}
