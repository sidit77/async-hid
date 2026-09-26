use std::ffi::c_void;
use std::future::{poll_fn, Future};
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::ptr::NonNull;
use std::slice::from_raw_parts;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use block2::RcBlock;
use crossbeam_queue::ArrayQueue;
use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained, GlobalQueueIdentifier};
use log::trace;
use objc2_core_foundation::{CFIndex, CFNumber, CFRetained};
use objc2_io_kit::{kIOHIDMaxInputReportSizeKey, kIOReturnBadArgument, kIOReturnSuccess, IOHIDDevice, IOHIDReportType, IOOptionBits, IOReturn};

use crate::backend::iohidmanager::context::CallbackContext;
use crate::backend::iohidmanager::device_info::property_key;
use crate::{ensure, AsyncHidFeatureHandle, AsyncHidRead, AsyncHidWrite, HidError, HidResult};

pub struct DeviceReadWriter {
    device: CFRetained<IOHIDDevice>,
    read_state: Option<ReaderState>,
    writable: bool,
    /// Explicit report transactions run here, one at a time per handle. It is
    /// not the queue the device delivers its input reports on, so a report in
    /// flight never delays a read.
    report_queue: DispatchRetained<DispatchQueue>,
}

unsafe impl Send for DeviceReadWriter {}
unsafe impl Sync for DeviceReadWriter {}

struct ReaderState {
    inner: *const AsyncReportReaderInner,
    report_buffer: ManuallyDrop<Vec<u8>>,
}

/// A device handle that may be moved to another thread.
///
/// # Safety
///
/// Two things make this sound. CoreFoundation reference counts are atomic, so
/// retaining and releasing an `IOHIDDevice` from a dispatch worker is defined,
/// including the case where the block holds the last reference because the
/// `DeviceReadWriter` was dropped while a report was still queued. And
/// `IOHIDDeviceGetReport` and `IOHIDDeviceSetReport`, the only things called
/// through this handle, are synchronous and carry no run loop or queue
/// affinity, unlike the callback-driven calls, which belong to the queue the
/// device was attached to.
struct SendDevice(CFRetained<IOHIDDevice>);
unsafe impl Send for SendDevice {}

/// Result of one dispatched report transaction.
///
/// Shared between the dispatched job and the waiting future, and nothing else.
/// IOKit never sees it: the native call only ever touches storage the job owns
/// outright, so this carries no lifetime obligation towards native code. A
/// dropped future simply releases its reference; the job keeps the allocation
/// alive and its result is discarded with it.
#[derive(Default)]
struct ReportCompletion {
    result: Mutex<Option<HidResult<Vec<u8>>>>,
    done: AtomicBool,
    waker: AtomicWaker,
}

impl ReportCompletion {
    fn complete(&self, outcome: HidResult<Vec<u8>>) {
        if let Ok(mut slot) = self.result.lock() {
            *slot = Some(outcome);
        }
        // Release, so the result stored above is visible to the acquiring load
        // in poll.
        self.done.store(true, Ordering::Release);
        self.waker.wake();
    }
}

/// Waits for one dispatched report transaction.
struct ReportCompletionFuture(Arc<ReportCompletion>);

impl Future for ReportCompletionFuture {
    type Output = HidResult<Vec<u8>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.waker.register(cx.waker());
        if !self.0.done.load(Ordering::Acquire) {
            return Poll::Pending;
        }
        Poll::Ready(match self.0.result.lock() {
            Ok(mut slot) => slot.take().unwrap_or_else(|| Err(HidError::message("report result taken twice"))),
            Err(e) => Err(HidError::message(format!("Mutex error: {:?}", e))),
        })
    }
}

/// Runs `job` on `queue` and hands its result to the returned future.
///
/// The seam the tests use: they substitute a job that sleeps, errors or returns
/// a short report, which exercises the ownership and cancellation behaviour
/// without IOKit.
fn dispatch_report_job<F>(queue: &DispatchQueue, job: F) -> ReportCompletionFuture
where
    F: Send + FnOnce() -> HidResult<Vec<u8>> + 'static,
{
    let completion = Arc::new(ReportCompletion::default());
    let job_completion = completion.clone();
    queue.exec_async(move || job_completion.complete(job()));
    ReportCompletionFuture(completion)
}

/// Room for report data in a caller buffer whose first byte carries the report
/// id. A zero id is not part of the report and is not sent, so the data occupies
/// the rest of the buffer; any other id is part of the report itself. This is
/// the same split the write path applies to the outgoing report.
fn report_capacity(buf_len: usize, report_id: u8) -> usize {
    match report_id {
        // A buffer too short to hold the id is rejected by reading it.
        0x0 => buf_len.saturating_sub(1),
        _ => buf_len,
    }
}

/// The buffer one native report transaction writes into.
///
/// One byte longer than a request of zero bytes needs. Such a request still goes
/// to the device, as it did before this path became an owned job, but an empty
/// `Vec` would hand IOKit a dangling pointer for it; this way the pointer
/// addresses an allocation even though the length tells IOKit that nothing may
/// be written through it. The requested capacity, not the length of this buffer,
/// stays the bound the device's answer is checked against.
fn native_buffer(capacity: usize) -> Vec<u8> {
    vec![0u8; capacity.max(1)]
}

/// Turns the outcome of one native `IOHIDDeviceGetReport` into a result.
///
/// `owned` is the buffer the call filled, `length` the number of bytes the device
/// claims to have written and `capacity` what was asked for. The length comes
/// from the device, so anything it cannot mean - negative, or more than was
/// requested - is an error rather than something to bend into a slice length.
fn report_from_native(ret: IOReturn, mut owned: Vec<u8>, length: CFIndex, capacity: usize) -> HidResult<Vec<u8>> {
    #[allow(non_upper_case_globals)]
    match ret {
        kIOReturnSuccess => match usize::try_from(length) {
            Ok(length) if length <= capacity => {
                owned.truncate(length);
                Ok(owned)
            }
            _ => Err(HidError::message(format!(
                "the device reported {length} bytes for a request of {capacity}"
            ))),
        },
        // IOKit answers a device that has gone away with a bad argument. The
        // calls that catch the removal itself may still return not responding or
        // not ready, which stay message errors, as they do on the write path.
        other if other == kIOReturnBadArgument as IOReturn => Err(HidError::Disconnected),
        other => Err(HidError::message(format!("failed to get report: {:#X}", other))),
    }
}

/// Hands a finished report transaction to the caller.
///
/// Only a successful report touches `buf`, and only as far as it reaches: the
/// bytes behind it keep whatever the caller left there.
fn copy_report_out(buf: &mut [u8], report_id: u8, report: HidResult<Vec<u8>>) -> HidResult<usize> {
    let report = report?;
    let target = match report_id {
        0x0 => &mut buf[1..],
        _ => buf,
    };
    let length = report.len().min(target.len());
    target[..length].copy_from_slice(&report[..length]);
    Ok(length)
}

unsafe impl Send for ReaderState {}
unsafe impl Sync for ReaderState {}

impl DeviceReadWriter {
    pub const DEVICE_OPTIONS: IOOptionBits = 0;

    pub fn new(device: CFRetained<IOHIDDevice>, dispatch_queue: &DispatchQueue, read: bool, write: bool) -> HidResult<Self> {
        if read || write {
            ensure!(
                device.open(DeviceReadWriter::DEVICE_OPTIONS) == kIOReturnSuccess,
                HidError::message("Failed to open device")
            );
        }

        let max_input_report_len = match read {
            false => None,
            true => {
                let len = device
                    .property(&property_key(kIOHIDMaxInputReportSizeKey))
                    .and_then(|p| p.downcast_ref::<CFNumber>().and_then(|n| n.as_i32()));
                match len {
                    Some(len) => Some(len as usize),
                    None => {
                        device.close(Self::DEVICE_OPTIONS);
                        return Err(HidError::message("Failed to read input report size"));
                    }
                }
            }
        };

        // Once a device is associated with a dispatch queue it must go through
        // activate + cancel before it can be released — dropping it early leaks
        // the dispatch machinery, because the mach channel created by
        // IOHIDDeviceSetDispatchQueue retains the device through its event
        // handler block (a retain cycle only IOHIDDeviceCancel breaks). Attach
        // the queue only after every fallible step above, so error paths drop a
        // queue-less device, which is safe to release as-is. The cancel side of
        // the contract lives in Drop.
        unsafe { device.set_dispatch_queue(dispatch_queue) };

        let read_state = max_input_report_len.map(|max_input_report_len| unsafe {
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
        });

        // Targeted at a user initiated global queue rather than given a QoS
        // floor afterwards: the floor may only be set while the object is
        // still inactive, and a queue from `new` is already active.
        let target = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated));
        let report_queue = DispatchQueue::new_with_target("async-hid-reports", DispatchQueueAttr::SERIAL, Some(&target));

        device.activate();

        Ok(Self {
            device,
            read_state,
            writable: write,
            report_queue,
        })
    }

    /// Common function to write reports from the specified [`IOHIDReportType`]
    async fn write_report<'a>(&'a self, report_type: IOHIDReportType, buf: &'a [u8]) -> HidResult<()> {
        assert!(self.writable, "Device is not writable");
        let report_id = buf[0];
        let data_to_send = if report_id == 0x0 { &buf[1..] } else { buf };

        // The synchronous SetReport runs on the device's own serial queue and is
        // awaited, so the signature stays asynchronous and no executor thread is
        // blocked. IOHIDDeviceSetReportWithCallback is deliberately not used: on
        // macOS it stops input report delivery after a few hundred calls.
        //
        // There is no timeout. SetReport offers none, and a crate that does not
        // pick a runtime has no timer of its own to race it against.
        let context = CallbackContext::<()>::new();
        let inner = context.inner();

        // The block outlives this frame as far as the type system knows, so it
        // gets owned copies. IOKit only reads the report, and owning it is what
        // makes a future dropped mid write cost a wasted write and nothing else.
        let mut data = data_to_send.to_vec();
        let device = SendDevice(self.device.clone());

        self.report_queue.exec_async(move || {
            // Force whole-struct capture (edition 2021+ disjoint capture).
            let device = device;
            let ret = unsafe {
                device.0.set_report(
                    report_type,
                    report_id as _,
                    NonNull::new_unchecked(data.as_mut_ptr()),
                    data.len() as _,
                )
            };
            inner.ret.store(ret, Ordering::Relaxed);
            // Release, so the return code above is visible to the acquiring
            // load in poll. Without it a completed write can be seen before the
            // code it completed with, and a failure reads as success.
            inner.done.store(true, Ordering::Release);
            // Signalling a future that is already gone writes to live memory
            // and wakes nobody: the block holds its own reference.
            inner.waker.wake();
        });

        context.await.map(|_| ())
    }

    /// Common function to read reports from the specified [`IOHIDReportType`]
    /// This is only for Output for Feature type reports.
    ///
    /// The native call runs synchronously inside a dispatched job that owns
    /// both the report buffer and the length cell, so nothing IOKit can reach
    /// depends on this future staying alive. Dropping the future detaches the
    /// waiter; it does not cancel the native operation, and the job's result is
    /// then discarded.
    async fn read_report<'a>(&'a self, report_type: IOHIDReportType, buf: &'a mut [u8]) -> HidResult<usize> {
        // Should never reach here for report types other that feature or output
        match report_type {
            IOHIDReportType::Feature | IOHIDReportType::Output => {}
            _ => panic!("Invalid read report type"),
        }

        let _ = self.read_state.as_ref().expect("Device is not readable");
        let report_id = buf[0];
        let capacity = report_capacity(buf.len(), report_id);

        let device = SendDevice(self.device.clone());
        let report = dispatch_report_job(&self.report_queue, move || {
            // Force whole-struct capture (edition 2021+ disjoint capture).
            let device = device;
            let mut owned = native_buffer(capacity);
            let mut length: CFIndex = capacity as CFIndex;
            // SAFETY: both pointers address storage owned by this closure, and
            // the call is synchronous, so IOKit cannot touch either after it
            // returns. `owned` is initialised, so no uninitialised byte is ever
            // exposed even if the device writes fewer bytes than requested.
            let ret = unsafe {
                device.0.report(
                    report_type,
                    report_id as _,
                    NonNull::new_unchecked(owned.as_mut_ptr()),
                    NonNull::new_unchecked(&mut length),
                )
            };
            report_from_native(ret, owned, length, capacity)
        })
        .await;

        copy_report_out(buf, report_id, report)
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
        self.read_report(IOHIDReportType::Feature, buf).await
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
        let this: &Self = unsafe { &*(context as *mut Self) };
        let mut buffer = this.empty_buffers.pop().unwrap_or_default();
        buffer.resize(report_length as usize, 0);
        buffer.copy_from_slice(unsafe { from_raw_parts(report.as_ptr(), report_length as usize) });
        if let Some(old) = this.full_buffers.force_push(buffer) {
            this.recycle_buffer(old);
        }
        this.waker.wake();
    }

    unsafe extern "C-unwind" fn hid_removal_callback(context: *mut c_void, _result: IOReturn, _sender: *mut c_void) {
        let this: &Self = unsafe { &*(context as *mut Self) };
        this.removed.store(true, Ordering::Relaxed);
        this.waker.wake();
    }
}
