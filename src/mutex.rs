//! src/mutex.rs

use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut, Drop};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};

use atomic_wait::{wait, wake_one}; //, wake_all};

pub struct Mutex<T> {
    /// 0: Unlocked
    /// 1: Locked, no other threads waiting
    /// 2: Locked, other threads waiting
    state: AtomicU32,
    value: UnsafeCell<T>,
}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            state: AtomicU32::new(0), // starts unlocked
            value: UnsafeCell::new(value),
        }
    }

    #[inline] // compiler hint to copy/paste this code into areas where it's called
    pub fn lock(&self) -> MutexGuard<'_, T> {
        // first attempt to change state from 0 to 1 (unlocked to first locked)
        if self.state.compare_exchange(0, 1, Acquire, Relaxed).is_err() {
            // if that fails, the mutex must already be locked
            // we will farm this out to a routine that will spin for a short period,
            // then resort to calling wait() if required.
            lock_contended(&self.state);
        }
        MutexGuard { mutex: self }
    }
}

#[cold] // compiler optimization hint letting it know that this won't be called in the
// uncontested case (hopefully common)
fn lock_contended(state: &AtomicU32) {
    let mut spin_count = 0;

    // we avoid a compare_and_exchange since MESI will generally attempt
    // to acquire exclusive access to the cache line, slowing things significantly.
    while state.load(Relaxed) == 1 && spin_count < 100 {
        spin_count += 1;
        std::hint::spin_loop(); // complier optimization hint
    }

    if state.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
        // we did it! no syscall required!
        return;
    }

    while state.swap(2, Acquire) != 0 {
        // spinning wasn't enough, let's take a rest until state changes to
        // something other than 2
        wait(state, 2);
    }
}

unsafe impl<T> Sync for Mutex<T> where T: Send {}

impl<T> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // this will help us skip the wake_one if no other threads are waiting for the lock
        if self.mutex.state.swap(0, Release) == 2 {
            // only perform is the previous value was 2
            wake_one(&self.mutex.state);
        }
    }
}

pub struct MutexGuard<'a, T> {
    pub(crate) mutex: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.value.get() }
    }
}
