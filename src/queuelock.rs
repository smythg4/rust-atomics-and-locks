//! src/queuelock.rs

use std::ptr;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
use std::sync::atomic::{AtomicBool, AtomicPtr};

struct McsNode {
    locked: AtomicBool, // true for "wait", false for "unlocked"
    next: AtomicPtr<McsNode>,
}
pub struct McsLock {
    tail: AtomicPtr<McsNode>,
}

impl McsLock {
    pub fn new() -> Self {
        McsLock {
            tail: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub fn lock(&self, node: &mut McsNode) {
        node.next.store(ptr::null_mut(), Relaxed);
        node.locked.store(true, Relaxed);

        // Swap ourselves into the tail
        let prev = self.tail.swap(node, AcqRel);

        if !prev.is_null() {
            // Someone is ahead of us -- link and spin
            unsafe { (*prev).next.store(node, Release) };

            while node.locked.load(Acquire) {
                std::hint::spin_loop();
            }
        }
    }

    pub fn unlock(&self, node: &mut McsNode) {
        let next = node.next.load(Acquire);

        if next.is_null() {
            // Maybe no one waiting - try to clear tail
            if self
                .tail
                .compare_exchange(node, ptr::null_mut(), Release, Relaxed)
                .is_ok()
            {
                return;
            }

            // CAS failed - someone is linking, wait for them
            while node.next.load(Acquire).is_null() {
                std::hint::spin_loop();
            }
        }

        // Signal next thread
        unsafe { (*node.next.load(Relaxed)).locked.store(false, Release) };
    }
}
