#![doc = include_str!("../README.md")]

mod backend;
mod device_info;
mod error;
mod traits;
mod utils;

/// All available backends for the current platform
pub use device_info::{Device, DeviceEvent, DeviceId, DeviceInfo, HidBackend};
pub use traits::{AsyncHidRead, AsyncHidWrite, HidOperations};

pub use crate::error::{HidError, HidResult};
pub use crate::backend::Backend;