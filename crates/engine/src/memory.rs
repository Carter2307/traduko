//! Giving memory back to the system.
//!
//! A loaded model is hundreds of megabytes; Traduko drops it after ten idle
//! minutes. With the system allocator alone that changes little: macOS keeps
//! the freed pages in the process, to reuse them or to reclaim them later
//! (measured on macOS 27: 464 MB before and after dropping the accurate
//! model). An app that sits on the desktop all day must not stay as heavy as
//! its last translation, so large blocks get their own pages from the
//! kernel, and return them the moment they are freed.

use std::alloc::{GlobalAlloc, Layout, System};

/// Use as the program's allocator:
///
/// ```ignore
/// #[global_allocator]
/// static ALLOCATOR: traduko_engine::ReturnsLargeBlocks = traduko_engine::ReturnsLargeBlocks;
/// ```
pub struct ReturnsLargeBlocks;

/// Blocks of this size or more are mapped and unmapped one by one. Model
/// weights are far above it; the small tensors of a translation step are
/// below it and keep the fast path.
const LARGE: usize = 64 * 1024;
/// Mapped memory starts on a page boundary, which is alignment enough for
/// anything up to a page.
const PAGE: usize = 16 * 1024;

fn is_large(layout: Layout) -> bool {
    layout.size() >= LARGE && layout.align() <= PAGE
}

fn whole_pages(size: usize) -> usize {
    size.div_ceil(PAGE) * PAGE
}

unsafe impl GlobalAlloc for ReturnsLargeBlocks {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !is_large(layout) {
            return unsafe { System.alloc(layout) };
        }
        // SAFETY: an anonymous private mapping with no address hint.
        let block = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                whole_pages(layout.size()),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if block == libc::MAP_FAILED { std::ptr::null_mut() } else { block.cast() }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if is_large(layout) {
            // Fresh pages from the kernel are already zero.
            unsafe { self.alloc(layout) }
        } else {
            unsafe { System.alloc_zeroed(layout) }
        }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        if is_large(layout) {
            // SAFETY: `block` came from `mmap` above with this same size.
            unsafe { libc::munmap(block.cast(), whole_pages(layout.size())) };
        } else {
            unsafe { System.dealloc(block, layout) };
        }
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return std::ptr::null_mut();
        };
        if !is_large(layout) && !is_large(new_layout) {
            return unsafe { System.realloc(block, layout, new_size) };
        }
        // One side of the move is mapped: copy across.
        let moved = unsafe { self.alloc(new_layout) };
        if !moved.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(block, moved, layout.size().min(new_size));
                self.dealloc(block, layout);
            }
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_and_small_blocks_hold_their_data_through_a_resize() {
        let allocator = ReturnsLargeBlocks;
        for (from, to) in [(1_000usize, 200_000usize), (200_000, 1_000), (100_000, 5_000_000), (5_000_000, 70_000), (10, 20)] {
            let layout = Layout::from_size_align(from, 8).unwrap();
            unsafe {
                let block = allocator.alloc(layout);
                assert!(!block.is_null());
                for i in 0..from.min(to) {
                    *block.add(i) = (i % 251) as u8;
                }
                let moved = allocator.realloc(block, layout, to);
                assert!(!moved.is_null());
                for i in (0..from.min(to)).step_by(97) {
                    assert_eq!(*moved.add(i), (i % 251) as u8, "byte {i} changed going from {from} to {to}");
                }
                allocator.dealloc(moved, Layout::from_size_align(to, 8).unwrap());
            }
        }
    }

    #[test]
    fn a_large_zeroed_block_is_zero() {
        let allocator = ReturnsLargeBlocks;
        let layout = Layout::from_size_align(300_000, 16).unwrap();
        unsafe {
            let block = allocator.alloc_zeroed(layout);
            assert!((0..300_000).step_by(1_013).all(|i| *block.add(i) == 0));
            allocator.dealloc(block, layout);
        }
    }
}
