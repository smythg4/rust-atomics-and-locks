//! src/examples/ticket_sales.rs
//! Demo of semaphore use

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::semaphore::Semaphore;

const NUM_TICKETS: u32 = 35;
const NUM_SELLERS: usize = 4;

struct TicketCounter {
    tickets: UnsafeCell<u32>,
    lock: Semaphore,
}

unsafe impl Sync for TicketCounter {}

impl TicketCounter {
    fn new(tickets: u32) -> Self {
        Self {
            tickets: UnsafeCell::new(tickets),
            lock: Semaphore::new(1),
        }
    }

    fn try_sell(&self) -> Option<u32> {
        let _permit = self.lock.acquire();

        // Safe: we hold the semaphore
        let tickets = unsafe { &mut *self.tickets.get() };

        if *tickets == 0 {
            None
        } else {
            *tickets -= 1;
            Some(*tickets)
        }
    }
}

pub fn run_ticket_sales() {
    let counter = Arc::new(TicketCounter::new(NUM_TICKETS));

    thread::scope(|s| {
        for id in 0..NUM_SELLERS {
            let counter = Arc::clone(&counter);
            s.spawn(move || {
                let mut sold_by_me = 0;

                loop {
                    // simulate working with customer
                    random_delay(50, 200);

                    match counter.try_sell() {
                        Some(remaining) => {
                            sold_by_me+=1;
                            println!("Seller #{id} sold one ({remaining} left)");
                        },
                        None => {
                            println!("Seller #{id} noticed all tickets sold! (I sold {sold_by_me} myself)");
                            break;
                        }
                    }
                }

            });
        }
    });

    println!("All done!");
}

fn random_delay(min_ms: u64, max_ms: u64) {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as u64;
    let delay = min_ms + (seed % (max_ms - min_ms));
    thread::sleep(Duration::from_millis(delay));
}
