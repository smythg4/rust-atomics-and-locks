//! src/examples/mod.rs

mod dining_philosophers;
mod ticket_sales;

pub use ticket_sales::run_ticket_sales;
pub use dining_philosophers::run_dining_philosophers;