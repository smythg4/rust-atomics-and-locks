//! src/semaphore.rs

use atomic_wait::{wait, wake_one};
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release}; //, wake_all};

pub struct Permit<'a> {
    pub(crate) semaphore: &'a Semaphore,
}

pub struct Semaphore {
    /// 0 indicates a locked state
    counter: AtomicU32,
}

impl Semaphore {
    pub fn new(permits: usize) -> Self {
        Self {
            counter: AtomicU32::new(permits as u32),
        }
    }

    pub fn acquire(&self) -> Permit<'_> {
        let mut count = self.counter.load(Relaxed);
        loop {
            if count > 0 {
                match self
                    .counter
                    .compare_exchange(count, count - 1, Acquire, Relaxed)
                {
                    Ok(_) => return Permit { semaphore: self },
                    Err(e) => count = e,
                }
            } else {
                acquire_contended(&self.counter);
                count = self.counter.load(Relaxed);
            }
        }
    }
}

#[cold]
fn acquire_contended(state: &AtomicU32) {
    // Spin briefly before sleeping
    let mut spin_count = 0;
    while state.load(Relaxed) == 0 && spin_count < 100 {
        spin_count += 1;
        std::hint::spin_loop();
    }
    let count = state.load(Relaxed);
    if count == 0 {
        wait(&state, 0);
    }
}

impl<'a> Drop for Permit<'a> {
    fn drop(&mut self) {
        if self.semaphore.counter.fetch_add(1, Release) == 0 {
            wake_one(&self.semaphore.counter);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::{Relaxed, SeqCst};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_semaphore_limits_concurrency() {
        let sem = Semaphore::new(2);
        let active = AtomicUsize::new(0);
        let max_active = AtomicUsize::new(0);

        thread::scope(|s| {
            for _ in 0..10 {
                s.spawn(|| {
                    let _permit = sem.acquire();

                    // Track concurrent access
                    let current = active.fetch_add(1, SeqCst) + 1;
                    max_active.fetch_max(current, SeqCst);

                    thread::sleep(Duration::from_millis(50));

                    active.fetch_sub(1, SeqCst);
                });
            }
        });

        // Should never exceed 2 concurrent
        assert!(max_active.load(Relaxed) <= 2);
        assert_eq!(active.load(Relaxed), 0);
    }
}
