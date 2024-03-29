#![warn(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;

thread_local!(static BYTES_ALLOCATED: RefCell<usize> = RefCell::new(0));

fn allocated_bytes(len: usize) {
    BYTES_ALLOCATED.with(|cell| {
        *cell.borrow_mut() += len;
    });
}

fn bytes_allocated() -> usize {
    BYTES_ALLOCATED.with(|cell| *cell.borrow())
}

struct ThreadStatsAlloc<A> {
    inner: A,
}

unsafe impl<A: GlobalAlloc> GlobalAlloc for ThreadStatsAlloc<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocated_bytes(layout.size());
        self.inner.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.dealloc(ptr, layout);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocated_bytes(layout.size());
        self.inner.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        allocated_bytes(new_size);
        self.inner.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: ThreadStatsAlloc<System> = ThreadStatsAlloc { inner: System };

pub fn assert_allocated_bytes<F, T>(bytes: usize, f: F) -> T
where
    F: FnOnce() -> T,
{
    let start = bytes_allocated();
    let result = f();
    let change = bytes_allocated() - start;
    assert_eq!(change, bytes);
    result
}

#[cfg(test)]
mod tests {
    use crate::assert_allocated_bytes;
    use std::mem;

    #[test]
    fn vec_allocates() {
        let mut v: Vec<u8> = Vec::new();
        assert_allocated_bytes(10, || {
            v.reserve(10);
        });
    }

    #[test]
    fn vec_reallocates() {
        let mut v: Vec<u8> = assert_allocated_bytes(5, || {
            let mut v = Vec::with_capacity(5);
            v.extend_from_slice(&[1, 2, 3, 4, 5]);
            v
        });

        assert_allocated_bytes(10, || {
            v.resize(10, 0u8);
        });
    }

    #[test]
    fn drop_does_not_allocate() {
        let v: Vec<u8> = assert_allocated_bytes(5, || {
            let mut v = Vec::with_capacity(5);
            v.extend_from_slice(&[1, 2, 3, 4, 5]);
            v
        });

        assert_allocated_bytes(0, || {
            mem::drop(v);
        });
    }
}
