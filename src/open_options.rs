use std::fmt::{Debug, Formatter};

use crate::device::DeviceFeatureHandle;
use crate::{Device, DeviceReader, DeviceReaderWriter, DeviceWriter, HidResult};

/// Whether other handles may use a device while this one holds it open.
///
/// Requested through [OpenOptions::access]. The request is strict: an open either provides
/// the requested mode, as defined per backend below, or fails, with
/// [HidError::Unsupported](crate::HidError::Unsupported) if the backend can not provide the mode
/// at all and with [HidError::Busy](crate::HidError::Busy) if the device is currently held by
/// another handle that excludes this one.
///
/// When no mode is requested, which is what [Device::open], [Device::open_readable],
/// [Device::open_writeable] and [Device::open_feature_handle] do, the platform default applies.
///
/// | Backend               | Platform default                              | `Shared`                          | `Exclusive`                                       |
/// |-----------------------|-----------------------------------------------|-----------------------------------|---------------------------------------------------|
/// | macOS (IOHIDManager)  | shared: `IOHIDDeviceOpen` with options `0`    | same as the default               | `IOHIDDeviceOpen` with `kIOHIDOptionsTypeSeizeDevice` |
/// | Windows (Win32)       | shared: `FILE_SHARE_READ \| FILE_SHARE_WRITE` | same as the default               | share mode `0`                                    |
/// | Windows (WinRT)       | not specified by WinRT                        | same as the default (see below)   | `Unsupported`                                     |
/// | Linux (hidraw)        | shared: the kernel allows any number of opens | same as the default               | `Unsupported`                                     |
/// | FreeBSD (hidraw)      | exclusive: the kernel allows a single open    | `Unsupported`                     | same as the default                               |
///
/// What `Exclusive` excludes, and what it does not, differs per backend; see [Access::Exclusive].
/// Statements marked *unverified* below have not been confirmed on hardware yet.
///
/// Further notes per backend:
///
/// - **macOS**: an open that IOKit refuses with `kIOReturnExclusiveAccess` is reported as `Busy`,
///   with the default access as well, and so is a report call that IOKit refuses with that code.
/// - **Windows (Win32)**: an open that fails with `ERROR_SHARING_VIOLATION` is reported as `Busy`,
///   with the default access as well.
/// - **Windows (WinRT)**: `HidDevice.FromIdAsync` takes no sharing argument and WinRT does not
///   document how it shares the device. `Shared` opens the device exactly like the default access does,
///   without any further guarantee.
/// - **Linux**: hidraw has no exclusive open. Advisory locks such as `flock(2)` are not enforced by the
///   kernel and are not used by this crate.
/// - **FreeBSD**: `hidraw_open` fails with `EBUSY` while the node is open, which is reported as `Busy`,
///   with the default access as well. Readers, writers and feature handles each open the node, so only
///   one of them can exist at a time, [Device::read_feature_report] and [HidBackend::query_devices](crate::HidBackend::query_devices)
///   fail with `Busy` while the device is open, and enumeration skips a device that is open.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Access {
    /// Other handles, from this or other processes, may open and use the device at the same time.
    ///
    /// On macOS, measured on 2026-10-11 with a YKUSH3 on Apple silicon (see [Access::Exclusive] for
    /// the setup):
    ///
    /// - every input report is delivered to every open shared or default handle, so a handle also
    ///   receives the answers to requests that other handles sent. A caller that sends a request and
    ///   reads the answer must check that the answer belongs to its request. Within one process and
    ///   across two processes.
    /// - `Shared` does not reserve the device: a later `Exclusive` open by another handle succeeds, and
    ///   this handle's output report writes fail with `Busy` while the seize is held. Once the seize is
    ///   released, this handle works again without being reopened. Within one process and across two
    ///   processes.
    Shared,
    /// No other handle can open the device for input or output report access while this one holds it.
    ///
    /// The exact scope per backend:
    ///
    /// - **macOS**: the device is seized at the IOHIDLib level (`IOHIDDeviceOpen` with
    ///   `kIOHIDOptionsTypeSeizeDevice`). Measured on 2026-10-11 with a YKUSH3 on macOS on Apple
    ///   silicon; each statement names its scope:
    ///   - while the seize is held, exclusive, shared and default opens are refused with `Busy`, and
    ///     the seizing handle keeps working. Within one process and across two processes.
    ///   - while the seize is held, the feature handle that [Device::read_feature_report] opens
    ///     internally is refused with `Busy`. Within one process only.
    ///   - dropping the seizing handle closes it and releases the seize. Within one process and across
    ///     two processes.
    ///   - a seize over an existing shared or default handle succeeds. That handle's output report
    ///     writes then fail with `kIOReturnExclusiveAccess` (`0xE00002C5`), reported as `Busy`; this is
    ///     an error, not a silent hang. After the seize is released, the handle works again without
    ///     being reopened. Within one process and across two processes.
    ///   - two shared handles coexist. Within one process and across two processes.
    ///   - not measured: what input report reads on a shared or default handle do while another handle
    ///     holds the seize. Only the writes were measured.
    ///
    ///   The measurement can be repeated with the ignored test in `tests/access.rs` (one process) and
    ///   the `examples/access_probe.rs` example (two processes); see the README.
    /// - **Windows (Win32)**: the handle is opened with share mode `0`, so other opens that request
    ///   read or write access fail with `ERROR_SHARING_VIOLATION` (`Busy`). Windows applies no sharing
    ///   check to handles that request neither, which is how enumeration, feature handles with the
    ///   default or shared access and [Device::read_feature_report] open a device. Those can still be
    ///   opened while the device is held exclusively and can read and write feature reports through
    ///   it. An exclusive feature handle is opened with read and write access so that its share mode
    ///   counts; this may fail with `ERROR_ACCESS_DENIED` on collections the system holds, such as
    ///   keyboards and mice, and the input reports the driver queues for it are never read.
    ///   *Unverified:* these statements follow the documented sharing rules of Windows; they have not
    ///   been checked on a Windows machine.
    /// - **FreeBSD**: every open of a hidraw node is exclusive at the hidraw level: any other open of
    ///   the same `/dev/hidrawN`, from this or another process, fails with `EBUSY` (`Busy`). The
    ///   guarantee covers opens of the hidraw node only, not other drivers that use the device.
    /// - **WinRT, Linux**: not available, the open fails with `Unsupported`.
    Exclusive
}

/// Options for opening a [Device], obtained from [Device::open_options].
///
/// Modelled after [std::fs::OpenOptions]: configure the options, then call one of the `open` methods,
/// which return the same handle types as the methods of the same name on [Device].
///
/// ```no_run
/// use async_hid::{Access, AsyncHidRead, Device, HidBackend, HidError, HidResult};
/// use futures_lite::StreamExt;
///
/// # async fn example() -> HidResult<()> {
/// let device = HidBackend::default()
///     .enumerate()
///     .await?
///     .find(|info: &Device| info.vendor_id == 0x04d8 && info.product_id == 0xf11b)
///     .await
///     .expect("Could not find device");
///
/// match device.open_options().access(Access::Exclusive).open().await {
///     Ok(mut handle) => {
///         let mut buffer = [0u8; 64];
///         let size = handle.read_input_report(&mut buffer).await?;
///         println!("{:?}", &buffer[..size]);
///     }
///     Err(HidError::Busy) => println!("The device is held by another handle"),
///     Err(HidError::Unsupported) => println!("This backend can not open devices exclusively"),
///     Err(err) => return Err(err)
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
#[must_use = "OpenOptions does nothing until one of its open methods is called"]
pub struct OpenOptions<'a> {
    device: &'a Device,
    access: Option<Access>
}

impl Debug for OpenOptions<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenOptions")
            .field("device", &self.device.id)
            .field("access", &self.access)
            .finish()
    }
}

impl<'a> OpenOptions<'a> {
    pub(crate) fn new(device: &'a Device) -> Self {
        Self { device, access: None }
    }

    /// Request the given [Access] mode.
    ///
    /// Without this call, the platform default described at [Access] applies.
    pub fn access(&mut self, access: Access) -> &mut Self {
        self.access = Some(access);
        self
    }

    /// Open the device in read-only mode
    pub async fn open_readable(&self) -> HidResult<DeviceReader> {
        self.device.open_readable_with(self.access).await
    }

    /// Open the device in write-only mode
    /// Note: Not all backends support this mode and might upgrade the permission to read+write behind the scenes
    pub async fn open_writeable(&self) -> HidResult<DeviceWriter> {
        self.device.open_writeable_with(self.access).await
    }

    /// Open the device in read and write mode
    pub async fn open(&self) -> HidResult<DeviceReaderWriter> {
        self.device.open_with(self.access).await
    }

    /// Open a handle for reading and writing feature reports
    pub async fn open_feature_handle(&self) -> HidResult<DeviceFeatureHandle> {
        self.device.open_feature_handle_with(self.access).await
    }
}
