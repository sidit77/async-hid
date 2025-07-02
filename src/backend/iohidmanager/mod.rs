mod device_info;
mod read_writer;

use std::collections::HashMap;
use std::ffi::c_void;
use std::pin::Pin;
use std::ptr::{null, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Once};
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use block2::RcBlock;
use crossbeam_queue::ArrayQueue;
use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use futures_lite::stream::{iter, Boxed};
use futures_lite::{Stream, StreamExt};
use log::{debug, trace, warn};
use objc2_core_foundation::{CFDictionary, CFRetained};
use objc2_io_kit::{
    kIOMasterPortDefault, IOHIDDevice, IOHIDManager, IOHIDManagerOptions, IORegistryEntryIDMatching, IOReturn, IOServiceGetMatchingService,
};

use crate::backend::iohidmanager::device_info::{get_device_id, get_device_info};
use crate::backend::iohidmanager::read_writer::DeviceReadWriter;
use crate::backend::{Backend, DeviceInfoStream};
use crate::utils::TryIterExt;
use crate::{ensure, DeviceEvent, DeviceId, DeviceInfo, HidError, HidResult};

static DISPATCH_QUEUE: LazyLock<DispatchRetained<DispatchQueue>> = LazyLock::new(|| DispatchQueue::new("async-hid", DispatchQueueAttr::SERIAL));

// TODO:
// - Async Read implementation

pub type IoHidManagerBackend = Arc<IoHidManagerBackendInner>;

pub struct IoHidManagerBackendInner {
    manager: CFRetained<IOHIDManager>,
    callback_context: *const ManagerCallbackContext,
}

//SAFETY: IOHIDManager is immediately connected to a dispatch queue, and
// all functions are called on that queue to the best of my knowledge
unsafe impl Send for IoHidManagerBackendInner {}
unsafe impl Sync for IoHidManagerBackendInner {}

impl Default for IoHidManagerBackendInner {
    fn default() -> Self {
        unsafe {
            trace!("Creating manager");
            let manager = IOHIDManager::new(None, IOHIDManagerOptions::None.bits());
            manager.set_dispatch_queue(&DISPATCH_QUEUE);

            let context = Box::into_raw(Box::new(ManagerCallbackContext::default()));
            manager.set_device_matching(None);
            manager.register_device_matching_callback(Some(ManagerCallbackContext::added_callback), context as *mut c_void);
            manager.register_device_removal_callback(Some(ManagerCallbackContext::removed_callback), context as *mut c_void);
            trace!("Scheduling manager with run loop");
            manager.activate();

            Self {
                manager,
                callback_context: context as _,
            }
        }
    }
}

impl IoHidManagerBackendInner {
    fn callback_context(&self) -> &ManagerCallbackContext {
        unsafe { &*self.callback_context }
    }
}

impl Drop for IoHidManagerBackendInner {
    fn drop(&mut self) {
        unsafe {
            let once = Arc::new(Once::new());
            let block = RcBlock::new({
                let once = once.clone();
                move || once.call_once(|| trace!("Finished canceling manager"))
            });

            self.manager.set_cancel_handler(RcBlock::as_ptr(&block));
            self.manager.cancel();
            trace!("Waiting for manager cancel to finish");
            once.wait();
            trace!("Resuming destructor of manager");

            //SAFETY: Cancel will ensure that the callbacks are never called again, allowing us to free the data structures reference in them
            drop(Box::from_raw(self.callback_context as *mut ManagerCallbackContext));
            self.callback_context = null();
        }
    }
}

impl Backend for IoHidManagerBackend {
    type Reader = Arc<DeviceReadWriter>;
    type Writer = Arc<DeviceReadWriter>;

    async fn enumerate(&self) -> HidResult<DeviceInfoStream> {
        let device_infos = unsafe {
            let device_set = self.manager.devices().expect("Failed to get devices");
            let mut devices: Vec<NonNull<IOHIDDevice>> = vec![NonNull::dangling(); device_set.count() as usize];
            device_set.values(devices.as_mut_ptr().cast());
            devices
                .iter()
                .map(|d| get_device_info(d.as_ref()))
                .try_flatten()
                .collect::<Vec<_>>()
        };
        Ok(iter(device_infos).boxed())
    }

    fn watch(&self) -> HidResult<Boxed<DeviceEvent>> {
        let watcher = DeviceWatcher::new(self.clone());
        Ok(watcher.boxed())
    }

    async fn query_info(&self, id: &DeviceId) -> HidResult<Vec<DeviceInfo>> {
        let device = get_device(id, None)?;
        let device_info = get_device_info(&device)?;
        Ok(device_info)
    }

    async fn open(&self, id: &DeviceId, read: bool, write: bool) -> HidResult<(Option<Self::Reader>, Option<Self::Writer>)> {
        let device = get_device(id, Some(&*DISPATCH_QUEUE))?;
        let rw = Arc::new(DeviceReadWriter::new(device, read, write)?);
        Ok((read.then_some(rw.clone()), write.then_some(rw)))
    }
}

fn get_device(id: &DeviceId, dispatch_queue: Option<&DispatchQueue>) -> HidResult<CFRetained<IOHIDDevice>> {
    unsafe {
        let service = IOServiceGetMatchingService(
            kIOMasterPortDefault,
            IORegistryEntryIDMatching(*id).map(|d| d.downcast::<CFDictionary>().unwrap()),
        );
        ensure!(service != 0, HidError::NotConnected);
        let device = IOHIDDevice::new(None, service).ok_or(HidError::message("Failed to create device"))?;
        if let Some(queue) = dispatch_queue {
            device.set_dispatch_queue(queue);
        }
        Ok(device)
    }
}

pub struct DeviceWatcher {
    id: u64,
    queue: Arc<AsyncQueue<DeviceEvent>>,
    backend: IoHidManagerBackend,
}

impl DeviceWatcher {
    pub fn new(backend: IoHidManagerBackend) -> Self {
        let (id, queue) = backend.callback_context().register_watcher();
        Self { id, queue, backend }
    }
}

impl Stream for DeviceWatcher {
    type Item = DeviceEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.queue.poll_next(cx)
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        self.backend.callback_context().unregister_watcher(self.id);
    }
}

#[derive(Default)]
struct ManagerCallbackContext {
    next_id: AtomicU64,
    watchers: Mutex<Vec<(u64, Arc<AsyncQueue<DeviceEvent>>)>>,
    devices: Mutex<HashMap<NonNull<IOHIDDevice>, DeviceId>>,
}

impl ManagerCallbackContext {
    pub fn register_watcher(&self) -> (u64, Arc<AsyncQueue<DeviceEvent>>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let queue = Arc::new(AsyncQueue::new(64));
        let mut watchers = self.watchers.lock().unwrap();
        watchers.push((id, queue.clone()));
        trace!("Registered watcher with id {} (total: {})", id, watchers.len());
        (id, queue)
    }

    pub fn unregister_watcher(&self, id: u64) {
        let mut watchers = self.watchers.lock().unwrap();
        watchers.retain(|&(i, _)| i != id);
        trace!("Unregistered watcher with id {} (remaining: {})", id, watchers.len());
    }

    fn notify_watchers(&self, event: DeviceEvent) {
        let mut watchers = self.watchers.lock().unwrap();
        for (_, queue) in watchers.iter_mut() {
            queue.force_push(event.clone());
        }
    }

    unsafe extern "C-unwind" fn added_callback(context: *mut c_void, _result: IOReturn, _sender: *mut c_void, device: NonNull<IOHIDDevice>) {
        let this: &Self = &*(context as *const Self);
        match get_device_id(device.as_ref()) {
            Ok(id) => {
                if let Some(prev_id) = this.devices.lock().unwrap().insert(device, id.clone()) {
                    warn!("Device {:p} connected with {:?} already has a stored device id {:?}", device, id, prev_id);
                }
                this.notify_watchers(DeviceEvent::Connected(id));
            }
            Err(err) => debug!("Failed to get device id: {}", err),
        }
    }

    unsafe extern "C-unwind" fn removed_callback(context: *mut c_void, _result: IOReturn, _sender: *mut c_void, device: NonNull<IOHIDDevice>) {
        let this: &Self = &*(context as *const Self);
        let device_id = this.devices.lock().unwrap().remove(&device);
        match device_id {
            Some(id) => this.notify_watchers(DeviceEvent::Disconnected(id)),
            None => debug!("Device disconnected but ID not found"),
        }
    }
}

pub struct AsyncQueue<T> {
    items: ArrayQueue<T>,
    waker: AtomicWaker,
}

impl<T> AsyncQueue<T> {
    pub fn new(cap: usize) -> Self {
        Self {
            items: ArrayQueue::new(cap),
            waker: AtomicWaker::new(),
        }
    }

    pub fn force_push(&self, item: T) {
        self.items.force_push(item);
        self.waker.wake();
    }

    pub fn poll_next(&self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.waker.register(cx.waker());
        self.items
            .pop()
            .map(Some)
            .map(Poll::Ready)
            .unwrap_or(Poll::Pending)
    }
}
