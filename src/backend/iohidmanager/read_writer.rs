use std::ffi::c_void;
use std::future::{poll_fn, Future};
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::ptr::NonNull;
use std::slice::from_raw_parts;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once, PoisonError};
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use block2::RcBlock;
use crossbeam_queue::ArrayQueue;
use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained, GlobalQueueIdentifier};
use log::trace;
use objc2_core_foundation::{CFIndex, CFNumber, CFRetained};
use objc2_io_kit::{kIOHIDMaxInputReportSizeKey, kIOReturnBadArgument, kIOReturnSuccess, IOHIDDevice, IOHIDReportType, IOOptionBits, IOReturn};

use crate::backend::iohidmanager::device_info::property_key;
use crate::{ensure, AsyncHidFeatureHandle, AsyncHidRead, AsyncHidWrite, HidError, HidResult};

pub struct DeviceReadWriter {
    device: CFRetained<IOHIDDevice>,
    read_state: Option<ReaderState>,
    writable: bool,
    /// A dropped feature or output report read, taken over by the next matching read.
    pending_read: PendingSlot,
    /// Runs GetReport and SetReport, apart from the queue that delivers input reports.
    report_queue: DispatchRetained<DispatchQueue>
}

unsafe impl Send for DeviceReadWriter {}
unsafe impl Sync for DeviceReadWriter {}

struct ReaderState {
    inner: *const AsyncReportReaderInner,
    report_buffer: ManuallyDrop<Vec<u8>>,
}

unsafe impl Send for ReaderState {}
unsafe impl Sync for ReaderState {}

/// A device handle that may be moved to a dispatch worker.
///
/// # Safety
///
/// CoreFoundation reference counts are atomic, and the synchronous
/// `IOHIDDeviceGetReport` and `IOHIDDeviceSetReport` have no queue affinity.
struct SendDevice(CFRetained<IOHIDDevice>);
unsafe impl Send for SendDevice {}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Nothing panics while one of these locks is held.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Completion<T> {
    result: Mutex<Option<HidResult<T>>>,
    waker: AtomicWaker
}

/// Waits for a job on the report queue. Dropping it does not stop the job.
struct CompletionFuture<T>(Arc<Completion<T>>);

impl<T> Future for CompletionFuture<T> {
    type Output = HidResult<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.waker.register(cx.waker());
        match lock(&self.0.result).take() {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending
        }
    }
}

/// Runs `job` on `queue`. The job must own everything the native call touches,
/// because it runs to completion even if the future is dropped.
fn dispatch<T, F>(queue: &DispatchQueue, job: F) -> CompletionFuture<T>
where
    T: Send + 'static,
    F: Send + FnOnce() -> HidResult<T> + 'static
{
    let completion = Arc::new(Completion {
        result: Mutex::new(None),
        waker: AtomicWaker::new()
    });
    let job_completion = completion.clone();
    queue.exec_async(move || {
        *lock(&job_completion.result) = Some(job());
        job_completion.waker.wake();
    });
    CompletionFuture(completion)
}

fn check(ret: IOReturn) -> HidResult<()> {
    #[allow(non_upper_case_globals)]
    match ret {
        kIOReturnSuccess => Ok(()),
        // IOKit answers a removed device with a bad argument.
        other if other == kIOReturnBadArgument as IOReturn => Err(HidError::Disconnected),
        other => Err(HidError::message(format!("report transaction failed: {:#X}", other)))
    }
}

/// The length comes from the device, so it is checked before it becomes a slice length.
fn report_from_native(ret: IOReturn, mut report: Vec<u8>, length: CFIndex, capacity: usize) -> HidResult<Vec<u8>> {
    check(ret)?;
    match usize::try_from(length) {
        Ok(length) if length <= capacity => {
            report.truncate(length);
            Ok(report)
        }
        _ => Err(HidError::message(format!(
            "the device reported {length} bytes for a request of {capacity}"
        )))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReportRequest {
    report_type: IOHIDReportType,
    report_id: u8,
    capacity: usize
}

struct PendingRead {
    request: ReportRequest,
    completion: CompletionFuture<Vec<u8>>
}

type PendingSlot = Mutex<Option<PendingRead>>;

fn forget_pending_read(slot: &PendingSlot) {
    lock(slot).take();
}

/// A pending read taken out of the slot, so that each completion has one waiter.
/// Dropped before the result arrives, it goes back for the next read.
struct ClaimedRead<'a> {
    slot: &'a PendingSlot,
    pending: Option<PendingRead>
}

impl Drop for ClaimedRead<'_> {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            // A clone of the handle may have left its own read meanwhile; keep that one.
            lock(self.slot).get_or_insert(pending);
        }
    }
}

/// Reads one report, taking over a pending read of the same request if there is one.
async fn read_resumable<F>(slot: &PendingSlot, queue: &DispatchQueue, request: ReportRequest, job: F) -> HidResult<Vec<u8>>
where
    F: Send + FnOnce() -> HidResult<Vec<u8>> + 'static
{
    let mut claimed = ClaimedRead {
        slot,
        pending: lock(slot)
            .take()
            .filter(|pending| pending.request == request)
    };
    let pending = claimed.pending.get_or_insert_with(|| PendingRead {
        request,
        completion: dispatch(queue, job)
    });
    let report = (&mut pending.completion).await;
    claimed.pending = None;
    report
}

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

        // A QoS floor can only be set on an inactive queue, so target a global queue instead.
        let target = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated));
        let report_queue = DispatchQueue::new_with_target("async-hid-reports", DispatchQueueAttr::SERIAL, Some(&target));

        device.activate();

        Ok(Self {
            device,
            read_state,
            writable: write,
            pending_read: Mutex::new(None),
            report_queue
        })
    }

    /// Common function to write reports from the specified [`IOHIDReportType`]
    async fn write_report<'a>(&'a self, report_type: IOHIDReportType, buf: &'a [u8]) -> HidResult<()> {
        assert!(self.writable, "Device is not writable");
        let report_id = buf[0];
        let mut data = if report_id == 0x0 { &buf[1..] } else { buf }.to_vec();

        // A report read before this write must not answer a read after it.
        forget_pending_read(&self.pending_read);

        // Not IOHIDDeviceSetReportWithCallback: on macOS it stops input report
        // delivery after a few hundred calls.
        let device = SendDevice(self.device.clone());
        dispatch(&self.report_queue, move || {
            let device = device;
            check(unsafe {
                device.0.set_report(
                    report_type,
                    report_id as _,
                    NonNull::new_unchecked(data.as_mut_ptr()),
                    data.len() as _,
                )
            })
        })
        .await
    }

    /// Common function to read reports from the specified [`IOHIDReportType`]
    /// This is only for Output for Feature type reports.
    async fn read_report<'a>(&'a self, report_type: IOHIDReportType, buf: &'a mut [u8]) -> HidResult<usize> {
        // Should never reach here for report types other that feature or output
        match report_type {
            IOHIDReportType::Feature | IOHIDReportType::Output => {}
            _ => panic!("Invalid read report type"),
        }

        let _ = self.read_state.as_ref().expect("Device is not readable");
        let report_id = buf[0];
        let target = if report_id == 0x0 { &mut buf[1..] } else { buf };
        let capacity = target.len();
        let request = ReportRequest {
            report_type,
            report_id,
            capacity
        };

        let device = SendDevice(self.device.clone());
        let report = read_resumable(&self.pending_read, &self.report_queue, request, move || {
            let device = device;
            // Never empty, an empty Vec would hand IOKit a dangling pointer.
            let mut report = vec![0u8; capacity.max(1)];
            let mut length = capacity as CFIndex;
            let ret = unsafe {
                device.0.report(
                    report_type,
                    report_id as _,
                    NonNull::new_unchecked(report.as_mut_ptr()),
                    NonNull::new_unchecked(&mut length)
                )
            };
            report_from_native(ret, report, length, capacity)
        })
        .await?;

        target[..report.len()].copy_from_slice(&report);
        Ok(report.len())
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::time::Duration;

    use futures_lite::future::{block_on, poll_once};

    use super::*;

    fn report_queue() -> DispatchRetained<DispatchQueue> {
        DispatchQueue::new("async-hid-report-tests", DispatchQueueAttr::SERIAL)
    }

    fn wait_for(what: &str, signal: &Receiver<()>) {
        signal
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
    }

    /// Blocks the queue until the returned sender is dropped.
    fn occupy(queue: &DispatchQueue) -> Sender<()> {
        let (release, wait_for_release) = channel::<()>();
        let (report_blocked, blocked) = channel::<()>();
        queue.exec_async(move || {
            let _ = report_blocked.send(());
            let _ = wait_for_release.recv();
        });
        wait_for("the queue to be occupied", &blocked);
        release
    }

    fn request(report_id: u8) -> ReportRequest {
        ReportRequest {
            report_type: IOHIDReportType::Feature,
            report_id,
            capacity: 8
        }
    }

    /// A read dropped after its first poll, like one that lost a race against a timeout.
    fn abandon(slot: &PendingSlot, queue: &DispatchQueue, request: ReportRequest, job: impl Send + FnOnce() -> HidResult<Vec<u8>> + 'static) {
        let mut read = Box::pin(read_resumable(slot, queue, request, job));
        assert!(block_on(poll_once(&mut read)).is_none(), "the read must still be waiting");
    }

    #[test]
    fn a_job_completing_before_the_first_poll_is_not_a_lost_wakeup() {
        let queue = report_queue();
        let (report_done, done) = channel::<()>();
        let mut job = dispatch(&queue, move || {
            let _ = report_done.send(());
            Ok(7)
        });
        wait_for("the job", &done);
        // The next block on the serial queue runs only once the result is stored.
        drop(occupy(&queue));

        assert!(matches!(block_on(poll_once(&mut job)), Some(Ok(7))));
    }

    /// A write must reach the device even if its future is gone.
    #[test]
    fn a_dropped_future_does_not_stop_its_job() {
        let queue = report_queue();
        let blocker = occupy(&queue);
        let (report_ran, ran) = channel::<()>();
        let mut job = dispatch(&queue, move || {
            let _ = report_ran.send(());
            Ok(())
        });
        assert!(block_on(poll_once(&mut job)).is_none());
        drop(job);
        drop(blocker);

        wait_for("the job of a dropped future", &ran);
    }

    #[test]
    fn a_read_dropped_by_a_timeout_hands_its_report_to_the_next_read() {
        let queue = report_queue();
        let slot = PendingSlot::default();
        let blocker = occupy(&queue);

        abandon(&slot, &queue, request(0x05), || Ok(vec![0x11; 4]));
        abandon(&slot, &queue, request(0x05), || panic!("the device was asked twice"));
        drop(blocker);
        let report = block_on(read_resumable(&slot, &queue, request(0x05), || panic!("the device was asked twice")));

        assert_eq!(report.expect("report"), vec![0x11; 4]);
        assert!(lock(&slot).is_none(), "a delivered report is no longer pending");
    }

    #[test]
    fn a_pending_read_for_another_request_is_not_handed_over() {
        let queue = report_queue();
        let slot = PendingSlot::default();
        let mut other_size = request(0x05);
        other_size.capacity = 4;

        for other in [request(0x06), other_size] {
            let blocker = occupy(&queue);
            abandon(&slot, &queue, request(0x05), || Ok(vec![0x11; 4]));
            drop(blocker);
            let report = block_on(read_resumable(&slot, &queue, other, || Ok(vec![0x22; 4])));
            assert_eq!(report.expect("report"), vec![0x22; 4]);
        }
    }

    #[test]
    fn a_forgotten_read_is_not_handed_over() {
        let queue = report_queue();
        let slot = PendingSlot::default();
        let blocker = occupy(&queue);

        abandon(&slot, &queue, request(0x05), || Ok(vec![0x11; 4]));
        forget_pending_read(&slot);
        drop(blocker);
        let report = block_on(read_resumable(&slot, &queue, request(0x05), || Ok(vec![0x22; 4])));

        assert_eq!(report.expect("report"), vec![0x22; 4]);
    }

    #[test]
    fn an_occupied_slot_keeps_its_read() {
        let queue = report_queue();
        let slot = PendingSlot::default();
        let blocker = occupy(&queue);

        let mut first = Box::pin(read_resumable(&slot, &queue, request(0x05), || Ok(vec![0x11; 4])));
        assert!(block_on(poll_once(&mut first)).is_none());
        abandon(&slot, &queue, request(0x05), || Ok(vec![0x22; 4]));
        drop(first);
        drop(blocker);
        let report = block_on(read_resumable(&slot, &queue, request(0x05), || Ok(vec![0x33; 4])));

        assert_eq!(report.expect("report"), vec![0x22; 4]);
    }

    #[test]
    fn an_impossible_native_length_is_an_error() {
        for (length, capacity) in [(-1, 16), (CFIndex::MIN, 16), (17, 16), (1, 0)] {
            let answer = report_from_native(kIOReturnSuccess, vec![0u8; capacity.max(1)], length, capacity);
            assert!(matches!(answer, Err(HidError::Message(_))), "{length} for {capacity} was accepted");
        }
        for (length, capacity) in [(16, 16), (4, 16), (0, 16), (0, 0)] {
            let report = report_from_native(kIOReturnSuccess, vec![0u8; capacity.max(1)], length, capacity);
            assert_eq!(report.expect("report").len(), length as usize);
        }
    }

    /// Codes measured on a YKUSH3 pulled during a read loop.
    #[test]
    fn the_code_a_removed_device_answers_with_is_a_disconnect() {
        assert!(matches!(check(0xE00002C2u32 as IOReturn), Err(HidError::Disconnected)));
        for code in [0xE00002EDu32, 0xE00002D8] {
            assert!(matches!(check(code as IOReturn), Err(HidError::Message(_))));
        }
    }
}
