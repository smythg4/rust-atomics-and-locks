//! src/examples/dining_philosophers.rs
//! demo use of semaphore

use std::thread;
use std::time::Duration;
use crate::semaphore::Semaphore;

const NUM_DINERS: usize = 5;
const EAT_TIMES: usize = 3;

pub fn run_dining_philosophers() {
    // One fork between each pair of philosophers
    let forks: Vec<Semaphore> = (0..NUM_DINERS)
        .map(|_| Semaphore::new(1))
        .collect();

    // Restrict to N-1 eaters to prevent deadlock
    let num_eating = Semaphore::new(NUM_DINERS - 1);

    thread::scope(|s| {
        for id in 0..NUM_DINERS {
            let left_fork = &forks[id];
            let right_fork = &forks[(id+1) % NUM_DINERS];
            let num_eating = &num_eating;

            s.spawn(move || {
                for _ in 0..EAT_TIMES {
                    think(id);
                    eat(id, num_eating, left_fork, right_fork);
                }
            });
        }
    });

    println!("All done!");
}

fn think(id: usize) {
    println!("Philosopher {id} thinking...");
    random_delay(100, 500);
}

fn eat(id: usize, num_eating: &Semaphore, left_fork: &Semaphore, right_fork: &Semaphore) {
    let _eating_permit = num_eating.acquire(); // wait for chance to try
    let _left = left_fork.acquire();
    let _right = right_fork.acquire();

    println!("Philosopher {id} eating...");
    random_delay(100, 500);
}

  fn random_delay(min_ms: u64, max_ms: u64) {                                                                    
      let seed = std::time::SystemTime::now()                                                                    
          .duration_since(std::time::UNIX_EPOCH)                                                                 
          .unwrap()                                                                                              
          .subsec_nanos() as u64;                                                                                
      let delay = min_ms + (seed % (max_ms - min_ms));                                                           
      thread::sleep(Duration::from_millis(delay));                                                               
  } 