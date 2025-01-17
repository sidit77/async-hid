use std::cell::UnsafeCell;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};

/// A simple mutex implementation with a guard that implements [Send].
/// SAFETY: The guard of the std mutex is not [Send] because pthread mutexes must only be unlocked from the thread that locked them.
/// SAFETY: This mutex is only backed by a single atomic bool, so it is safe to unlock from any thread.

pub struct SimpleMutex<T: ?Sized>{
    lock: Lock,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Send for SimpleMutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for SimpleMutex<T> {}

#[must_use = "if unused the Mutex will immediately unlock"]
pub struct SimpleMutexGuard<'a, T: ?Sized + 'a> {
    lock: &'a SimpleMutex<T>
}

impl<T> SimpleMutex<T> {
    #[inline]
    pub const fn new(t: T) -> SimpleMutex<T> {
        SimpleMutex { lock: Lock::new(), data: UnsafeCell::new(t) }
    }
}

impl<T: ?Sized> SimpleMutex<T> {
    pub fn try_lock(&self) -> Option<SimpleMutexGuard<'_, T>> {
        self.lock.try_lock().then(|| SimpleMutexGuard { lock: self })
    }

}

impl<T: ?Sized + Debug> Debug for SimpleMutex<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Mutex");
        match self.try_lock() {
            Some(guard) => s.field("data", &&*guard),
            None => s.field("data", &format_args!("<locked>"))
        };
        s.finish_non_exhaustive()
    }
}

impl<T: ?Sized> Deref for SimpleMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for SimpleMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for SimpleMutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.lock.lock.unlock();
    }
}

impl<T: ?Sized + Debug> Debug for SimpleMutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + Display> Display for SimpleMutexGuard<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&**self, f)
    }
}

#[derive(Debug)]
#[repr(transparent)]
struct Lock(AtomicBool);

impl Lock {
    const fn new() -> Lock {
        Lock(AtomicBool::new(false))
    }

    fn try_lock(&self) -> bool {
        !self.0.swap(true, Ordering::Acquire)
    }

    fn unlock(&self) {
        self.0.store(false, Ordering::Release);
    }
}