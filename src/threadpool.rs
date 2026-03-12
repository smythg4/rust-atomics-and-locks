use std::{
    thread::{self, JoinHandle},
    time::Duration,
};

type Task = Box<dyn FnOnce() + Send>;

fn worker_loop(task_rx: flume::Receiver<Task>, id: usize) {
    while let Ok(task) = task_rx.recv() {
        eprintln!("Worker {id} received a task...");
        task();
        eprintln!("Worker {id} going to sleep...");
        std::thread::sleep(Duration::from_secs(2));
        eprintln!("Worker {id} is awake and ready for tasking!");
    }
    eprintln!("Worker {id} ran out of jobs.");
}

pub struct ThreadPool {
    task_tx: Option<flume::Sender<Task>>,
    workers: Vec<JoinHandle<()>>,
}

impl ThreadPool {
    pub fn new(num_workers: usize) -> Self {
        let (tx, rx) = flume::bounded(10);
        let mut workers = Vec::with_capacity(num_workers);
        for i in 0..num_workers {
            eprintln!("Creating Worker {i}...");
            let rx_clone = rx.clone();
            let handle = thread::spawn(move || worker_loop(rx_clone, i));
            workers.push(handle);
        }
        Self {
            task_tx: Some(tx),
            workers,
        }
    }

    pub fn spawn(&self, f: impl FnOnce() + Send + 'static) {
        if let Some(tx) = &self.task_tx {
            tx.send(Box::new(f)).expect("failed to send");
        } else {
            panic!("No task sender found!");
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        // Close the sender channel
        let _ = self.task_tx.take();
        eprintln!("Sender closing...");
        for (i, worker) in self.workers.drain(..).enumerate() {
            eprintln!("Waiting on Worker {i}...");
            worker.join().unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_tasks_execute() {
        let expected = 100;
        let pool = ThreadPool::new(10);
        let counter = Arc::new(Mutex::new(0));
        for _ in 0..expected {
            let counter = Arc::clone(&counter);
            pool.spawn(move || {
                let mut n = counter.lock().unwrap();
                *n += 1;
                eprintln!("Counter Value: {n}");
            });
        }
        drop(pool); // waits for all tasks to finish
        assert_eq!(*counter.lock().unwrap(), expected);
    }
}
