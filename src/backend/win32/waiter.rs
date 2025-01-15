use std::ffi::c_void;
use std::future::Future;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use atomic_waker::AtomicWaker;
use log::trace;
use static_assertions::assert_not_impl_all;
use windows::Win32::Foundation::{BOOLEAN, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Threading::{RegisterWaitForSingleObject, UnregisterWaitEx, INFINITE, WT_EXECUTEINWAITTHREAD, WT_EXECUTEONLYONCE};
use crate::HidResult;


pub struct WaitableHandleFuture {
    waitable: HANDLE,
    registration: HANDLE,
    inner: WaitableHandleFutureInner
}

#[derive(Default)]
struct WaitableHandleFutureInner {
    waker: AtomicWaker,
    complete: AtomicBool,
    _unpin: PhantomPinned
}

impl WaitableHandleFuture {
    pub fn new(waitable: HANDLE) -> Self {
        Self {
            waitable,
            registration: INVALID_HANDLE_VALUE,
            inner: Default::default(),
        }
    }

    unsafe extern "system" fn callback_func(inner: *mut c_void, _: BOOLEAN) {
        trace!("Received wait callback");
        let inner = &*(inner as *const WaitableHandleFutureInner);
        inner.complete.store(true, Ordering::SeqCst);
        inner.waker.wake();
    }

}

assert_not_impl_all!(WaitableHandleFuture: Unpin);

impl Future for WaitableHandleFuture {
    type Output = HidResult<()>;

    fn poll(mut self: Pin<&mut WaitableHandleFuture>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.registration.is_invalid() {
            trace!("Registering waitable handle ({}) with the I/O thread pool", self.waitable.0);
            unsafe {
                RegisterWaitForSingleObject(
                    // SAFETY: Only the [WaitableHandleFutureInner] part of the [WaitableHandleFuture] must be !Unpin.
                    self.as_mut().map_unchecked_mut(|s| &mut s.registration).get_mut(),
                    self.waitable,
                    Some(Self::callback_func),
                    // SAFETY: The [WaitableHandleFutureInner] and therefore [WaitableHandleFuture] as a whole is !Unpin and the pointer should remain stable until `drop` has been called.
                    Some(&self.inner as *const _ as *mut c_void),
                    INFINITE,
                    WT_EXECUTEINWAITTHREAD | WT_EXECUTEONLYONCE)?
            };
        }
        {
            self.inner.waker.register(cx.waker());
            if self.inner.complete.load(Ordering::SeqCst) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }

    }
}

impl Drop for WaitableHandleFuture {
    fn drop(&mut self) {
        if !self.registration.is_invalid() {
            trace!("Unregistering waitable handle ({}) from the I/O thread pool", self.waitable.0);
            unsafe {
                // SAFETY: Calling `UnregisterWaitEx` with `INVALID_HANDLE_VALUE` will cancel the wait and wait for all callbacks functions to complete before returning.
                // Therefore, all pointers to the [WaitableHandleFutureInner] should be gone by the time this function returns.
                UnregisterWaitEx(self.registration, INVALID_HANDLE_VALUE)
                    .expect("Failed to cancel wait");
            }
            trace!("Waitable handle ({}) was successfully unregistered from the I/O thread pool", self.waitable.0);
        }
    }
}
