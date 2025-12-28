use std::{
    ffi::c_void,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicI32},
        Arc, Mutex,
    },
    task::Poll,
};

use atomic_waker::AtomicWaker;
use log::trace;

use crate::{HidError, HidResult};
use objc2_io_kit::kIOReturnSuccess;

/// Inner context passed to the callback. This is wrapped in an Arc,
/// so it maintains a stable pointer between the future and the callback.
///
/// The allocation will not be dropped until the callback drops the last Arc ref.
///
/// Interior mutability is required for all members of the inner context.
pub struct CallbackInner<Result> {
    /// Data to be returned by the future
    pub result: Mutex<Option<Result>>,

    /// Return code from IOHIDManager
    pub ret: AtomicI32,

    /// Async waker
    pub waker: AtomicWaker,

    /// Atomic flag to indicate the callback is done
    pub done: AtomicBool,

    /// Atomic flag to indicate the future was dropped.
    /// The callback function can check this and return
    pub cancelled: AtomicBool,
}

impl<Result> Default for CallbackInner<Result> {
    fn default() -> Self {
        Self {
            result: Default::default(),
            waker: Default::default(),
            done: Default::default(),
            cancelled: Default::default(),
            ret: Default::default(),
        }
    }
}

/// Callback Context Wrapper
pub struct CallbackContext<Result> {
    inner: Arc<CallbackInner<Result>>,
}

impl<Result> CallbackContext<Result> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CallbackInner::default()),
        }
    }

    /// Get a raw pointer to the inner context.
    /// This should be provided to the callback function
    #[inline(always)]
    pub fn as_raw(&self) -> *const CallbackInner<Result> {
        let callback_arc = self.inner.clone();
        Arc::into_raw(callback_arc)
    }

    pub fn inner_from_raw(raw: *const c_void) -> Arc<CallbackInner<Result>> {
        unsafe { Arc::from_raw(raw as *const CallbackInner<Result>) }
    }
}

impl<Result> Drop for CallbackContext<Result> {
    fn drop(&mut self) {
        // Set the cancelled flag to indicate to the callback the future
        // has gone out of scope and can release the Arc and return.
        self.inner
            .cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);

        trace!("CallbackContext dropped");
    }
}

impl<R: Copy> Future for CallbackContext<R> {
    type Output = HidResult<Option<R>>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.inner.waker.register(cx.waker());

        // Check if the callback has set the done flag
        if !self.inner.done.load(std::sync::atomic::Ordering::Relaxed) {
            return Poll::Pending;
        }

        // Check the return code
        #[allow(non_upper_case_globals, non_snake_case)]
        Poll::Ready(match self.inner.ret.load(std::sync::atomic::Ordering::Relaxed) {
            kIOReturnSuccess => match self.inner.result.lock() {
                Ok(result) => Ok(*result),
                Err(e) => Err(HidError::message(format!("Mutex error: {:?}", e))),
            },
            other => Err(HidError::message(format!("report writer callback error: {:#X}", other))),
        })
    }
}
