//! src/unboundedchannel.rs

//! An lock-free unbounded channel using a Michael-Scott queue.
//! Further optimization: Implement a lock-free ring buffer and allocate 'Blocks' for the
//! queue.

use crate::queue::Queue;
use atomic_wait::{wait, wake_all, wake_one};
use crossbeam_epoch as epoch;
use std::sync::Arc;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize};

struct Channel<T> {
    queue: Queue<T>,
    sender_count: AtomicUsize,
    receiver_alive: AtomicBool,
    // incremented on each push, used for wake signaling
    push_count: AtomicU32,
}

#[derive(Debug)]
pub enum SenderError<T> {
    ReceiverDropped(T),
}

pub struct Sender<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Sender<T> {
    pub fn send(&self, value: T) -> Result<(), SenderError<T>> {
        if !self.channel.receiver_alive.load(Acquire) {
            return Err(SenderError::ReceiverDropped(value));
        }
        let guard = &epoch::pin();
        self.channel.queue.push_back(value, guard);

        // Signal that something was pushed
        self.channel.push_count.fetch_add(1, Release);
        wake_one(&self.channel.push_count);
        Ok(())
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.channel.sender_count.fetch_add(1, Relaxed);
        Sender {
            channel: Arc::clone(&self.channel),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.channel.sender_count.fetch_sub(1, Release) == 1 {
            // last sender, wake receiver so it can see disconnection
            wake_all(&self.channel.push_count);
        }
    }
}

#[derive(Debug)]
pub enum ReceiverError {
    SendersDropped,
    Empty,
}

pub struct Receiver<T> {
    channel: Arc<Channel<T>>,
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, ReceiverError> {
        let guard = &epoch::pin();

        loop {
            // try to pop
            if let Some(value) = self.channel.queue.pop_front(guard) {
                return Ok(value);
            }

            // Queue empty -- check if senders still exist
            if self.channel.sender_count.load(Acquire) == 0 {
                // try one more time
                return self
                    .channel
                    .queue
                    .pop_front(guard)
                    .ok_or(ReceiverError::SendersDropped);
            }

            // Wait for a push
            let count = self.channel.push_count.load(Acquire);

            // Double check queue is still empty
            if let Some(value) = self.channel.queue.pop_front(guard) {
                return Ok(value);
            }

            // Still empty, sleep until push_count changes
            wait(&self.channel.push_count, count);
        }
    }

    pub fn try_recv(&self) -> Option<T> {
        let guard = &epoch::pin();
        self.channel.queue.pop_front(guard)
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.channel.receiver_alive.store(false, Release);
    }
}

pub fn ub_channel<T>() -> (Sender<T>, Receiver<T>) {
    let channel = Arc::new(Channel {
        queue: Queue::new(),
        sender_count: AtomicUsize::new(1),
        push_count: AtomicU32::new(0),
        receiver_alive: AtomicBool::new(true),
    });

    (
        Sender {
            channel: channel.clone(),
        },
        Receiver { channel },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_unbounded_channels_simple() {
        let (tx, rx) = ub_channel();
        let first_message = "Message from thread 1";
        let second_message = "Message from thread 2";

        let tx_clone = tx.clone();
        thread::scope(|s| {
            s.spawn(move || {
                thread::sleep(Duration::from_secs(1));
                tx_clone.send(first_message).ok();
            });
            s.spawn(move || {
                thread::sleep(Duration::from_secs(2));
                tx.send(second_message).ok();
            });
            s.spawn(|| {
                let msg1 = rx.recv().unwrap();
                let msg2 = rx.recv().unwrap();
                assert!(
                    (msg1 == first_message && msg2 == second_message)
                        || (msg1 == second_message && msg2 == first_message)
                );
            });
        });

        assert!(rx.recv().is_err());
    }

    #[test]
    fn test_ub_drain_after_disconnect() {
        let (tx, rx) = ub_channel();

        tx.send("a").unwrap();
        tx.send("b").unwrap();
        tx.send("c").unwrap();
        drop(tx); // sender gone, but buffer has items                                                            

        assert_eq!(rx.recv().unwrap(), "a");
        assert_eq!(rx.recv().unwrap(), "b");
        assert_eq!(rx.recv().unwrap(), "c");
        assert!(rx.recv().is_err()); // now it's empty and disconnected                                           
    }

    #[test]
    fn test_ub_sender_error_on_receiver_drop() {
        let (tx, rx) = ub_channel::<i32>();

        drop(rx);

        let result = tx.send(1);
        assert!(result.is_err());
    }
}
