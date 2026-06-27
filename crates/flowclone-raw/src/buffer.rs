//! A tiny buffer pool to reuse block buffers across copy iterations.

use std::sync::Mutex;

/// Pool of reusable byte buffers. Avoids re-allocating multi-MiB buffers on
/// every read/write step.
pub struct BufferPool {
    block_size: usize,
    free: Mutex<Vec<Vec<u8>>>,
}

impl BufferPool {
    pub fn new(capacity: usize, block_size: usize) -> Self {
        let mut free = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            free.push(vec![0u8; block_size]);
        }
        Self {
            block_size,
            free: Mutex::new(free),
        }
    }

    /// Acquire a buffer, allocating if the pool is empty.
    pub fn acquire(&self) -> Vec<u8> {
        let mut free = self.free.lock().expect("buffer pool poisoned");
        free.pop().unwrap_or_else(|| vec![0u8; self.block_size])
    }

    /// Return a buffer to the pool for reuse.
    pub fn release(&self, buf: Vec<u8>) {
        let mut free = self.free.lock().expect("buffer pool poisoned");
        free.push(buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_buffers() {
        let pool = BufferPool::new(1, 1024);
        let a = pool.acquire();
        pool.release(a);
        let b = pool.acquire();
        assert_eq!(b.len(), 1024);
    }

    #[test]
    fn allocates_when_empty() {
        let pool = BufferPool::new(0, 64);
        let buf = pool.acquire();
        assert_eq!(buf.len(), 64);
    }
}
