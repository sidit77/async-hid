use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicI32},
        Arc, Mutex,
    },
    task::Poll,
};

use atomic_waker::AtomicWaker;

use crate::{HidError, HidResult};
use objc2_io_kit::{kIOReturnBadArgument, kIOReturnSuccess};

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
}

impl<Result> Default for CallbackInner<Result> {
    fn default() -> Self {
        Self {
            result: Default::default(),
            waker: Default::default(),
            done: Default::default(),
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

    /// The shared state, for completions signalled from Rust rather than from
    /// an IOKit callback.
    pub fn inner(&self) -> Arc<CallbackInner<Result>> {
        self.inner.clone()
    }
}

/// The return code IOKit uses for a device that is no longer there. Cast once:
/// the constant is unsigned, the return code is read as signed.
const BAD_ARGUMENT: i32 = kIOReturnBadArgument as i32;

impl<R: Copy> Future for CallbackContext<R> {
    type Output = HidResult<Option<R>>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.inner.waker.register(cx.waker());

        // Check if the callback has set the done flag
        if !self.inner.done.load(std::sync::atomic::Ordering::Acquire) {
            return Poll::Pending;
        }

        // Check the return code
        #[allow(non_upper_case_globals, non_snake_case)]
        Poll::Ready(match self.inner.ret.load(std::sync::atomic::Ordering::Relaxed) {
            kIOReturnSuccess => match self.inner.result.lock() {
                Ok(result) => Ok(*result),
                Err(e) => Err(HidError::message(format!("Mutex error: {:?}", e))),
            },
            // IOKit answers a device that has gone away with a bad argument.
            // Mapped here rather than at one call site, so reads get it too.
            BAD_ARGUMENT => Err(HidError::Disconnected),
            other => Err(HidError::message(format!("report callback error: {:#X}", other))),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use futures_lite::future::block_on;

    use super::*;

    /// The write path completes the context from Rust rather than from an
    /// IOKit callback, so that route has to reach the future.
    #[test]
    fn a_context_completed_from_rust_yields_its_result() {
        let context = CallbackContext::<usize>::new();
        let inner = context.inner();

        *inner.result.lock().unwrap() = Some(7);
        inner.done.store(true, Ordering::Release);

        assert!(matches!(block_on(context), Ok(Some(7))));
    }

    #[test]
    fn a_bad_argument_is_reported_as_a_disconnect() {
        let context = CallbackContext::<()>::new();
        let inner = context.inner();

        inner.ret.store(BAD_ARGUMENT, Ordering::Relaxed);
        inner.done.store(true, Ordering::Release);

        assert!(matches!(block_on(context), Err(HidError::Disconnected)));
    }

    #[test]
    fn any_other_return_code_is_reported_as_an_error() {
        let context = CallbackContext::<()>::new();
        let inner = context.inner();

        inner.ret.store(-536870212, Ordering::Relaxed);
        inner.done.store(true, Ordering::Release);

        assert!(matches!(block_on(context), Err(HidError::Message(_))));
    }
}
