use atomics_and_locks::run_dining_philosophers;

fn main() {
    //run_ticket_sales();
    run_dining_philosophers();
}

#[cfg(test)]
mod tests {
    use atomics_and_locks::{Condvar, Mutex};
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    #[test]
    fn mutex_test() {
        let m = Mutex::new(0);
        std::hint::black_box(&m);
        let start = Instant::now();

        thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..5_000_000 {
                        *m.lock() += 1;
                    }
                });
            }
        });

        let duration = start.elapsed();
        println!("locked {} times in {:?}", *m.lock(), duration);
    }

    #[test]
    fn test_condvar() {
        let mutex = Mutex::new(0);
        let condvar = Condvar::new();

        let mut wakeups = 0;

        thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_secs(1));
                *mutex.lock() = 123;
                condvar.notify_one();
            });
            s.spawn(|| {
                thread::sleep(Duration::from_secs(1));
                *mutex.lock() = 123;
                condvar.notify_one();
            });
            let mut m = mutex.lock();
            while *m < 100 {
                m = condvar.wait(m);
                println!("Main thread woke up!");
                wakeups += 1;
            }

            assert_eq!(*m, 123);
        });

        // Check that the main thread actually did wait (not busy-loop),
        // while still allowing for a few spurious wake ups.
        assert!(wakeups < 10);
    }
}
