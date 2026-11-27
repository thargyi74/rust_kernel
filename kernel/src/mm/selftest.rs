// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: none exactly; like the page and `kmalloc` checks of
//! `lib/test_meminit.c`, run on the real allocators.

use crate::pr_info;
use alloc::boxed::Box;
use alloc::format;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use foundation::mm::{Pages, alloc_pages, free_pages, nr_free_pages};
use klib::mm::buddy::MAX_PAGE_ORDER;
use klib::prandom::XorShift64;

/// Blocks the random test holds at most.
const HELD: usize = 64;
/// Blocks one drain may hold. The drain takes the biggest blocks first, so
/// a few hundred blocks hold all of memory, not ~130,000 single pages. A
/// heap `Vec` would take its pages from the pool being drained, and the
/// boot stack is only 64 KiB.
const DRAINED: usize = 512;

/// Checks the page allocator once it holds all free memory.
pub fn page_alloc_selftest() {
    let start = nr_free_pages();
    // The blocks before anything is split. A top-down drain splits
    // nothing, so it also leaves the free lists as they were.
    let pristine = drain();
    assert_eq!(pristine.1, start, "page allocator self-test: drained pages != free pages");

    // 1. Random alloc/free steps, which split blocks, then free everything.
    let mut rng = XorShift64::new(7);
    let mut held: [Option<Pages>; HELD] = [const { None }; HELD];
    for _ in 0..2_000 {
        let slot = &mut held[rng.below(HELD as u64) as usize];
        match slot.take() {
            Some(pages) => free_pages(pages),
            None => *slot = alloc_pages(rng.below(MAX_PAGE_ORDER as u64 + 1) as u8),
        }
    }
    held.iter_mut().filter_map(Option::take).for_each(free_pages);
    assert_eq!(nr_free_pages(), start, "page allocator self-test: random steps lost pages");

    // 2. Freeing must have merged every split block back: a drain finds
    //    the same blocks as before step 1.
    assert_eq!(drain(), pristine, "page allocator self-test: blocks did not merge back");
    assert_eq!(nr_free_pages(), start, "page allocator self-test: draining lost pages");
    pr_info!("rust_kernel: page allocator self-test OK ({} pages)", pristine.1);
}

/// Allocates blocks from order `MAX_PAGE_ORDER` down to 0, each order until
/// `None`, checks that no page is left, frees them all and returns
/// (blocks, pages).
fn drain() -> (usize, u64) {
    let mut held: [Option<Pages>; DRAINED] = [const { None }; DRAINED];
    let (mut blocks, mut pages) = (0, 0);
    for order in (0..=MAX_PAGE_ORDER).rev() {
        while let Some(block) = alloc_pages(order) {
            let slot = held.get_mut(blocks).expect("page allocator self-test: more blocks than the drain holds");
            pages += block.size() / 4096;
            *slot = Some(block);
            blocks += 1;
        }
    }
    assert_eq!(nr_free_pages(), 0, "page allocator self-test: pages left after the drain");
    held.iter_mut().filter_map(Option::take).for_each(free_pages);
    (blocks, pages)
}

/// Checks the heap: the slab path, the page path and alignment.
pub fn heap_selftest() {
    let mut v: Vec<u32> = Vec::new();
    for i in 0..10_000 {
        v.push(i);
    }
    assert_eq!(v.iter().sum::<u32>(), 49_995_000, "heap self-test: Vec sum");
    let b = Box::new([7u8; 100]);
    assert!(b.iter().all(|&x| x == 7), "heap self-test: Box contents");
    let a = Arc::new(5u32);
    let a2 = Arc::clone(&a);
    assert_eq!(Arc::strong_count(&a), 2, "heap self-test: Arc strong_count");
    drop(a2);
    let s = format!("{}-{}", "heap", 42);
    assert_eq!(s, "heap-42", "heap self-test: format!");

    // The page path: more than the largest size class. Its pages go back.
    let before = nr_free_pages();
    {
        let big = vec![0xa5u8; 100_000];
        assert!(big.capacity() >= 100_000 && big.iter().all(|&x| x == 0xa5), "heap self-test: big Vec");
    }
    assert_eq!(nr_free_pages(), before, "heap self-test: big Vec pages not given back");

    #[repr(align(4096))]
    struct Aligned([u8; 16]);
    let al = Box::new(Aligned([1; 16]));
    assert_eq!(&*al as *const Aligned as usize % 4096, 0, "heap self-test: Box alignment");
    assert_eq!(al.0[15], 1);
    pr_info!("rust_kernel: heap self-test OK");
}
