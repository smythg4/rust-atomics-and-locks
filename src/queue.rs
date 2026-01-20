//! src/queue.rs

//! A lock-free queue to demonstrate RCU. Uses crossbeam-epoch for memory management.

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared, unprotected};
use std::mem::MaybeUninit;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};

struct Link<T> {
    data: MaybeUninit<T>,
    next: Atomic<Link<T>>,
}

impl<T> Link<T> {
    pub const fn new(value: T) -> Self {
        Link {
            data: MaybeUninit::new(value),
            next: Atomic::null(),
        }
    }

    const fn sentinel() -> Self {
        Link {
            data: MaybeUninit::uninit(),
            next: Atomic::null(), // this is const fn
        }
    }
}

// Progressing into a lock-free queue
pub struct Queue<T> {
    head: Atomic<Link<T>>,
    tail: Atomic<Link<T>>,
}

impl<T> Queue<T> {
    pub fn new() -> Self {
        // Sentinel node
        let sentinel = Owned::new(Link::sentinel());
        let guard = &epoch::pin();
        let sentinel = sentinel.into_shared(guard);

        Queue {
            head: Atomic::from(sentinel),
            tail: Atomic::from(sentinel),
        }
    }

    pub fn push_back(&self, value: T, guard: &Guard) {
        let new = Owned::new(Link::new(value));
        let new = Owned::into_shared(new, guard);

        loop {
            // push onto the tail, so look there first
            let tail = self.tail.load(Acquire, guard);

            // attempt to push onto the `tail` snapshot, fails if `tail.next` has changed.
            let tail_ref = unsafe { tail.deref() };
            let next = tail_ref.next.load(Acquire, guard);

            // If `tail` is not the actual tail, try to "help" by moving the tail pointer forward
            if !next.is_null() {
                let _ = self
                    .tail
                    .compare_exchange(tail, next, Release, Relaxed, guard);
                continue;
            }

            // looks like the actual tail; attempt to link at `tail.next`
            if tail_ref
                .next
                .compare_exchange(Shared::null(), new, Release, Relaxed, guard)
                .is_ok()
            {
                // attempt to move the tail pointer forward
                let _ = self
                    .tail
                    .compare_exchange(tail, new, Release, Relaxed, guard);
                break;
            }
        }
    }

    pub fn pop_front(&self, guard: &Guard) -> Option<T> {
        loop {
            let head = self.head.load(Acquire, guard);
            let head_ref = unsafe { head.deref() };
            let next = head_ref.next.load(Acquire, guard);
            if next.is_null() {
                return None;
            }
            let next_ref = unsafe { next.deref() };

            // Moves `tail` if it's stale. Relaxed load is enough because if tail == head, then
            // message for that node are already acquired
            let tail = self.tail.load(Relaxed, guard);
            if tail == head {
                let _ = self
                    .tail
                    .compare_exchange(tail, next, Release, Relaxed, guard);
            }

            if self
                .head
                .compare_exchange(head, next, Release, Relaxed, guard)
                .is_ok()
            {
                unsafe {
                    guard.defer_destroy(head);
                    return Some(std::ptr::read(&next_ref.data).assume_init());
                }
            }
        }
    }
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Queue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = &epoch::pin();
        let head = self.head.load(Relaxed, guard);
        let mut active_node = unsafe { head.deref().next.load(Relaxed, guard) }; // skips sentinel
        while !active_node.is_null() {
            let node_data = unsafe { &active_node.deref().data.assume_init_read() };
            write!(f, "{} -> ", node_data)?;
            active_node = unsafe { active_node.deref().next.load(Relaxed, guard) };
        }
        write!(f, "∅")
    }
}

impl<T> Drop for Queue<T> {
    fn drop(&mut self) {
        unsafe {
            let guard = unprotected();

            while self.pop_front(guard).is_some() {}

            // Destroy the sentinel node
            let sentinel = self.head.load(Relaxed, guard);
            drop(sentinel.into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_basics() {
        let guard = &epoch::pin();
        let list = Queue::new();
        list.push_back(1, guard);
        list.push_back(2, guard);
        list.push_back(3, guard);

        println!("{}", list);

        assert_eq!(
            unsafe {
                list.head
                    .load(Relaxed, guard)
                    .deref()
                    .next
                    .load(Relaxed, guard)
                    .deref()
                    .data
                    .assume_init_read()
            },
            1
        );

        let first = list.pop_front(guard);
        assert_eq!(Some(1), first);
        assert_eq!(
            unsafe {
                list.head
                    .load(Relaxed, guard)
                    .deref()
                    .next
                    .load(Relaxed, guard)
                    .deref()
                    .data
                    .assume_init_read()
            },
            2
        );
        println!("{}", list);

        let first = list.pop_front(guard);
        assert_eq!(Some(2), first);
        assert_eq!(
            unsafe {
                list.head
                    .load(Relaxed, guard)
                    .deref()
                    .next
                    .load(Relaxed, guard)
                    .deref()
                    .data
                    .assume_init_read()
            },
            3
        );
        println!("{}", list);

        let first = list.pop_front(guard);
        assert_eq!(Some(3), first);
        println!("{}", list);

        assert!(list.pop_front(guard).is_none());
    }

    #[test]
    fn test_queue_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let list = Arc::new(Queue::new());
        let num_threads = 4;
        let ops_per_thread = 1000;

        thread::scope(|s| {
            // Producers
            for t in 0..num_threads {
                let list = Arc::clone(&list);
                s.spawn(move || {
                    let guard = &epoch::pin();
                    let mut count = 0;
                    for i in 0..ops_per_thread {
                        list.push_back(t * ops_per_thread + i, guard);
                        count += 1;
                    }
                    println!("Producer sent {count} items");
                });
            }

            // Consumers
            for _ in 0..num_threads {
                let list = Arc::clone(&list);
                s.spawn(move || {
                    let guard = &epoch::pin();
                    let mut count = 0;
                    loop {
                        match list.pop_front(guard) {
                            Some(_) => count += 1,
                            None => {
                                // Yield and retry a few times before giving up
                                thread::yield_now();
                                if list.pop_front(guard).is_none() {
                                    break;
                                }
                            }
                        }
                    }
                    println!("Consumer got {count} items");
                });
            }
        });

        // Queue should be empty
        let guard = &epoch::pin();
        assert!(list.pop_front(guard).is_none());
    }
}
