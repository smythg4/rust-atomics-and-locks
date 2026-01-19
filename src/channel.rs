//! src/channel.rs

use crate::condvar::Condvar;
use crate::mutex::Mutex;
use std::sync::Arc;

struct Inner<T> {
    buffer: Vec<Option<T>>, // fixed size, slots are Some or None
    capacity: usize,
    head: usize,  // next read position
    tail: usize,  // next write position
    count: usize, // num items in the buffer
    sender_count: usize,
    receiver_alive: bool, // so sender knows if receiver dropped
}

pub struct Channel<T> {
    inner: Mutex<Inner<T>>,
    not_empty: Condvar, // receiver waits here
    not_full: Condvar,  // send waits here
}

pub struct Sender<T> {
    channel: Arc<Channel<T>>,
}

#[derive(Debug)]
pub enum SenderError {
    ReceiverDropped,
}

impl<T> Sender<T> {
    pub fn send(&self, value: T) -> Result<(), SenderError> {
        let mut guard = self.channel.inner.lock();
        while guard.count == guard.capacity && guard.receiver_alive {
            guard = self.channel.not_full.wait(guard);
        }
        if !guard.receiver_alive {
            return Err(SenderError::ReceiverDropped);
        }
        guard.count += 1;
        let tail = guard.tail;
        guard.buffer[tail] = Some(value);
        guard.tail = (tail + 1) % guard.capacity;
        self.channel.not_empty.notify_one();
        Ok(())
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut guard = self.channel.inner.lock();
        guard.sender_count -= 1;
        if guard.sender_count == 0 {
            drop(guard);
            self.channel.not_empty.notify_one();
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let mut guard = self.channel.inner.lock();
        guard.sender_count += 1;
        drop(guard);
        Self {
            channel: Arc::clone(&self.channel),
        }
    }
}

pub struct Receiver<T> {
    channel: Arc<Channel<T>>,
}

#[derive(Debug)]
enum ReceiverError {
    SendersDropped,
    Empty,
}

impl<T> Receiver<T> {
    pub fn recv(&self) -> Result<T, ReceiverError> {
        let mut guard = self.channel.inner.lock();
        while guard.count == 0 && guard.sender_count > 0 {
            guard = self.channel.not_empty.wait(guard);
        }
        if guard.count > 0 {
            let head = guard.head;
            let result = guard.buffer[head].take(); // replaces Some with None
            guard.head = (head + 1) % guard.capacity;
            guard.count -= 1;
            drop(guard);
            self.channel.not_full.notify_one();
            Ok(result.expect("buffer slot should be Some when count > 0"))
        } else {
            Err(ReceiverError::SendersDropped)
        }
    }

    pub fn try_recv(&self) -> Result<T, ReceiverError> {
        let mut guard = self.channel.inner.lock();
        if guard.count > 0 {
            let head = guard.head;
            let result = guard.buffer[head].take();
            guard.head = (head + 1) % guard.capacity;
            guard.count -= 1;
            drop(guard);
            self.channel.not_full.notify_one();
            Ok(result.expect("buffer slot should be Some when count > 0"))
        } else if guard.sender_count == 0 {
            Err(ReceiverError::SendersDropped)
        } else {
            Err(ReceiverError::Empty)
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut guard = self.channel.inner.lock();
        guard.receiver_alive = false;
        drop(guard);
        self.channel.not_full.notify_all(); // need ALL senders to give up since single receiver dropped
    }
}

pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let mut buffer = Vec::with_capacity(capacity);
    buffer.resize_with(capacity, || None);

    let channel = Arc::new(Channel {
        inner: Mutex::new(Inner {
            buffer,
            capacity,
            head: 0,
            tail: 0,
            count: 0,
            sender_count: 1,
            receiver_alive: true,
        }),
        not_empty: Condvar::new(),
        not_full: Condvar::new(),
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
    fn test_channels_simple() {
        let (tx, rx) = channel(2);
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
}
