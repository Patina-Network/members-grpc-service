use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Default)]
pub struct IdGenerator {
    next_id: AtomicU32,
}

impl IdGenerator {
    pub const fn default() -> Self {
        Self::new(0)
    }

    pub const fn new(start: u32) -> Self {
        Self {
            next_id: AtomicU32::new(start),
        }
    }

    pub fn next_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}
