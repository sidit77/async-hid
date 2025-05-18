use std::ffi::c_void;
use std::future::Future;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use atomic_waker::AtomicWaker;
use log::{trace, warn};
use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Threading::{RegisterWaitForSingleObject, UnregisterWaitEx, INFINITE, WT_EXECUTEINWAITTHREAD, WT_EXECUTEONLYONCE};

use crate::HidResult;

pub struct HandleWaiter {
    waitable: HANDLE,
    registration: HANDLE,
    inner: *const WaitableHandleFutureInner
}

unsafe impl Send for HandleWaiter {}
unsafe impl Sync for HandleWaiter {}

#[derive(Default)]
struct WaitableHandleFutureInner {
    waker: AtomicWaker,
    complete: AtomicBool,
    _unpin: PhantomPinned
}

impl HandleWaiter {
    pub fn new(waitable: HANDLE) -> Self {
        Self {
            waitable,
            registration: INVALID_HANDLE_VALUE,
            inner: Box::into_raw(Box::new(WaitableHandleFutureInner::default()))
        }
    }

    unsafe extern "system" fn callback_func(inner: *mut c_void, _: bool) {
        trace!("Received wait callback");
        let inner = &*(inner as *const WaitableHandleFutureInner);
        inner.complete.store(true, Ordering::SeqCst);
        inner.waker.wake();
    }

    fn is_registered(&self) -> bool {
        !self.registration.is_invalid()
    }

    fn register(&mut self) -> HidResult<()> {
        assert!(!self.is_registered());
        trace!("Registering waitable handle ({:p}) with the I/O thread pool", self.waitable.0);
        unsafe {
            RegisterWaitForSingleObject(
                &mut self.registration,
                self.waitable,
                Some(Self::callback_func),
                Some(self.inner as *mut c_void),
                INFINITE,
                WT_EXECUTEINWAITTHREAD | WT_EXECUTEONLYONCE
            )?
        };
        Ok(())
    }

    fn unregister(&mut self) -> HidResult<()> {
        assert!(self.is_registered());
        trace!("Unregistering waitable handle ({:p}) from the I/O thread pool", self.waitable.0);
        // Calling `UnregisterWaitEx` with `INVALID_HANDLE_VALUE` will cancel the wait and wait for all callbacks functions to complete before returning.
        unsafe {
            UnregisterWaitEx(self.registration, None)?;
        }
        self.registration = INVALID_HANDLE_VALUE;
        trace!(
            "Waitable handle ({:p}) was successfully unregistered from the I/O thread pool",
            self.waitable.0
        );
        Ok(())
    }

    fn reset(&mut self) -> HidResult<()> {
        if self.is_registered() {
            warn!("Waiter was not unregistered correctly in the previous call");
            self.unregister()?;
        }
        let inner = unsafe { &*self.inner };
        inner.complete.store(false, Ordering::SeqCst);
        inner.waker.take();
        Ok(())
    }

    pub fn wait(&mut self) -> HandleFuture<'_> {
        HandleFuture {
            inner: self,
            state: FutureState::Uninitialized
        }
    }
}

impl Drop for HandleWaiter {
    fn drop(&mut self) {
        if self.is_registered() {
            self.unregister()
                .expect("Failed to unregister waitable handle");
        }
        // SAFETY: Calling unregister removes all external references to self.inner
        drop(unsafe { Box::from_raw(self.inner as *mut WaitableHandleFutureInner) });
    }
}

#[derive(Debug, Eq, PartialEq)]
enum FutureState {
    Uninitialized,
    Initialized,
    Completed
}

pub struct HandleFuture<'a> {
    inner: &'a mut HandleWaiter,
    state: FutureState
}

impl<'a> Future for HandleFuture<'a> {
    type Output = HidResult<()>;

    fn poll(mut self: Pin<&mut HandleFuture<'a>>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        assert_ne!(self.state, FutureState::Completed);
        if self.state == FutureState::Uninitialized {
            self.inner.reset()?;
            self.inner.register()?;
            self.state = FutureState::Initialized;
        }
        {
            let inner = unsafe { &*self.inner.inner };
            inner.waker.register(cx.waker());
            if inner.complete.load(Ordering::SeqCst) {
                self.state = FutureState::Completed;
                self.inner.unregister()?;
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }
    }
}

impl Drop for HandleFuture<'_> {
    fn drop(&mut self) {
        if self.state == FutureState::Initialized && self.inner.is_registered() {
            self.inner
                .unregister()
                .expect("Failed to unregister waitable handle");
        }
    }
}
