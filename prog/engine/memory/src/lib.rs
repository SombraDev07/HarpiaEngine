//! CPU allocator wrapper. The engine does **not** ship a custom malloc.
//!
//! Bins install [`Allocator`] as `#[global_allocator]`. Libs never set a global
//! allocator. Validation callbacks can run on another thread; mimalloc has TLS.

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicU64, Ordering};

use mimalloc::MiMalloc;

/// Process-wide counters. Relaxed atomics: stats, not synchronization.
#[derive(Debug)]
pub struct Stats {
    pub alloc_count: AtomicU64,
    pub dealloc_count: AtomicU64,
    pub bytes_live: AtomicU64,
    pub bytes_allocated_total: AtomicU64,
}

impl Stats {
    pub const fn new() -> Self {
        Self {
            alloc_count: AtomicU64::new(0),
            dealloc_count: AtomicU64::new(0),
            bytes_live: AtomicU64::new(0),
            bytes_allocated_total: AtomicU64::new(0),
        }
    }

    fn on_alloc(&self, size: usize) {
        let n = size as u64;
        self.alloc_count.fetch_add(1, Ordering::Relaxed);
        self.bytes_live.fetch_add(n, Ordering::Relaxed);
        self.bytes_allocated_total.fetch_add(n, Ordering::Relaxed);
    }

    fn on_dealloc(&self, size: usize) {
        let n = size as u64;
        self.dealloc_count.fetch_add(1, Ordering::Relaxed);
        self.bytes_live.fetch_sub(n.min(self.bytes_live.load(Ordering::Relaxed)), Ordering::Relaxed);
    }
}

/// `GlobalAlloc` that forwards to mimalloc and updates [`STATS`].
pub struct Allocator {
    inner: MiMalloc,
}

pub static STATS: Stats = Stats::new();

impl Allocator {
    pub const fn new() -> Self {
        Self { inner: MiMalloc }
    }

    pub fn stats() -> &'static Stats {
        &STATS
    }
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { self.inner.alloc(layout) };
        if !p.is_null() {
            STATS.on_alloc(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        STATS.on_dealloc(layout.size());
        unsafe { self.inner.dealloc(ptr, layout) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { self.inner.alloc_zeroed(layout) };
        if !p.is_null() {
            STATS.on_alloc(layout.size());
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { self.inner.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            STATS.on_dealloc(layout.size());
            STATS.on_alloc(new_size);
        }
        p
    }
}

/// Frame bump. Wraps `bumpalo`. Reset once per CPU frame; do not use for GPU lifetimes.
pub struct FrameBump {
    inner: bumpalo::Bump,
}

impl FrameBump {
    pub fn new() -> Self {
        Self {
            inner: bumpalo::Bump::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn alloc<T>(&self, val: T) -> &T {
        self.inner.alloc(val)
    }

    pub fn allocated_bytes(&self) -> usize {
        self.inner.allocated_bytes()
    }
}

impl Default for FrameBump {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_reset_releases_for_reuse() {
        let mut bump = FrameBump::new();
        let _ = bump.alloc(1u64);
        assert!(bump.allocated_bytes() >= 8);
        bump.reset();
        let _ = bump.alloc(2u64);
    }
}
