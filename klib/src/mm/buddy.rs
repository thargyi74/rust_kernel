// SPDX-License-Identifier: GPL-2.0-only
//! The buddy page allocator. Linux counterpart: `mm/page_alloc.c`
//! (`__free_one_page`, `__rmqueue_smallest`, `expand`) with the buddy math of
//! `mm/page_alloc.h`.

//!
//! Simplified against Linux: one zone, no migrate types and no per-CPU page
//! lists. Free lists are linked through the frame array itself, by index, so
//! the allocator needs no memory of its own.

use crate::mm::addr::{PAGE_SIZE, Pfn, PhysAddr};
use core::num::NonZeroU32;

/// Largest block order: 2^10 pages = 4 MiB (`include/linux/mmzone.h:31`).
pub const MAX_PAGE_ORDER: u8 = 10;
pub const NR_PAGE_ORDERS: usize = MAX_PAGE_ORDER as usize + 1;

/// A frame index + 1, so `None` fits in the same 4 bytes.
type Link = Option<NonZeroU32>;

/// Most frames one allocator can describe: every index + 1 fits a `u32`.
const MAX_FRAMES: usize = u32::MAX as usize - 1;

/// What one page frame is doing (Linux keeps this in `struct page`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameState {
    /// Not managed: a hole, firmware, the kernel image, memblock allocations.
    Reserved,
    /// Head of a free block of 2^order pages, on the free list of `order`.
    Free { order: u8, prev: Link, next: Link },
    /// Head of a block handed out by `alloc`.
    Allocated { order: u8 },
    /// Any frame of a free or allocated block except its head.
    Tail,
}

/// The allocator's record of one page frame.
#[derive(Clone, Copy)]
pub struct Frame {
    state: FrameState,
}

const _: () = assert!(size_of::<Frame>() <= 16);

impl Frame {
    /// A frame the allocator does not manage. Fill new arrays with it.
    pub const RESERVED: Frame = Frame { state: FrameState::Reserved };
}

/// 2^order pages handed out by [`BuddyAllocator::alloc`]. It is neither
/// `Clone` nor `Copy`, so the safe API can free a block only once.
pub struct PageBlock {
    pfn: Pfn,
    order: u8,
}

impl PageBlock {
    pub fn pfn(&self) -> Pfn {
        self.pfn
    }

    pub fn order(&self) -> u8 {
        self.order
    }

    pub fn addr(&self) -> PhysAddr {
        self.pfn.addr()
    }

    /// Size in bytes.
    pub fn size(&self) -> u64 {
        PAGE_SIZE << self.order
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BuddyError {
    /// The frame is not the head of an allocated block of that order.
    NotAllocated,
}

/// Free lists of 2^0 .. 2^MAX_PAGE_ORDER pages over a frame array, where
/// `frames[i]` describes page frame `base + i` (Linux: `struct free_area`).
pub struct BuddyAllocator<'a> {
    frames: &'a mut [Frame],
    base: Pfn,
    heads: [Link; NR_PAGE_ORDERS],
    counts: [usize; NR_PAGE_ORDERS],
    free_pages: u64,
}

impl<'a> BuddyAllocator<'a> {
    /// An allocator with every frame reserved, so nothing to hand out yet.
    pub fn new(frames: &'a mut [Frame], base: Pfn) -> Self {
        let len = frames.len().min(MAX_FRAMES);
        let frames = frames.split_at_mut(len).0;
        frames.fill(Frame::RESERVED);
        BuddyAllocator { frames, base, heads: [None; NR_PAGE_ORDERS], counts: [0; NR_PAGE_ORDERS], free_pages: 0 }
    }

    /// Frees the reserved frames in `[start, end)`, in the largest aligned
    /// blocks, as `__free_pages_memory` does (`mm/memblock.c:2342-2349`).
    /// Frames outside the array, or not reserved, are left alone.
    pub fn add_free_range(&mut self, start: Pfn, end: Pfn) {
        let first = self.base.as_u64();
        let mut pfn = start.as_u64().max(first);
        let end = end.as_u64().min(first + self.frames.len() as u64);
        while pfn < end {
            let mut order = if pfn == 0 { MAX_PAGE_ORDER } else { (pfn.trailing_zeros() as u8).min(MAX_PAGE_ORDER) };
            while pfn + (1 << order) > end {
                order -= 1;
            }
            // Shrink the block until every frame in it is reserved; a lone
            // frame that is not is skipped.
            let i = (pfn - first) as usize;
            while order > 0 && !self.all_reserved(i, order) {
                order -= 1;
            }
            if self.all_reserved(i, order) {
                self.frames[i + 1..i + (1 << order)].fill(Frame { state: FrameState::Tail });
                self.free_block(pfn, order);
            }
            pfn += 1 << order;
        }
    }

    /// A block of 2^order pages from the smallest order that has one, split
    /// down as `__rmqueue_smallest` and `expand` do
    /// (`mm/page_alloc.c:1714-1924`). `None` if no block is big enough.
    pub fn alloc(&mut self, order: u8) -> Option<PageBlock> {
        if order > MAX_PAGE_ORDER {
            return None;
        }
        let mut current = (order..=MAX_PAGE_ORDER).find(|&o| self.heads[o as usize].is_some())?;
        let i = index_of(self.heads[current as usize]?);
        self.unlink(i, current);
        // Hand back the upper halves, one per order, as `expand` does.
        while current > order {
            current -= 1;
            self.push(i + (1 << current), current);
        }
        self.frames[i].state = FrameState::Allocated { order };
        self.free_pages -= 1 << order;
        Some(PageBlock { pfn: Pfn::new(self.base.as_u64() + i as u64), order })
    }

    /// Gives a block back and merges it with its free buddies.
    ///
    /// # Panics
    ///
    /// If `block` did not come from this allocator's `alloc`.
    pub fn free(&mut self, block: PageBlock) {
        if self.free_raw(block.pfn, block.order).is_err() {
            panic!("buddy: freeing a block this allocator did not hand out");
        }
    }

    /// [`free`](Self::free) for callers that kept only the frame number and
    /// order, such as `GlobalAlloc`. Checks that `pfn` heads an allocated
    /// block of `order`.
    pub fn free_raw(&mut self, pfn: Pfn, order: u8) -> Result<(), BuddyError> {
        match self.index(pfn.as_u64()) {
            Some(i) if self.frames[i].state == (FrameState::Allocated { order }) => {
                self.free_block(pfn.as_u64(), order);
                Ok(())
            }
            _ => Err(BuddyError::NotAllocated),
        }
    }

    /// Free pages in all orders.
    pub fn free_pages(&self) -> u64 {
        self.free_pages
    }

    /// Free blocks of exactly `order`.
    pub fn free_blocks(&self, order: u8) -> usize {
        self.counts.get(order as usize).copied().unwrap_or(0)
    }

    /// The index of frame `pfn`, if the array describes it.
    fn index(&self, pfn: u64) -> Option<usize> {
        let i = usize::try_from(pfn.checked_sub(self.base.as_u64())?).ok()?;
        (i < self.frames.len()).then_some(i)
    }

    fn all_reserved(&self, i: usize, order: u8) -> bool {
        self.frames[i..i + (1 << order)].iter().all(|f| f.state == FrameState::Reserved)
    }

    /// Puts the block at `pfn` on a free list, merging it with its buddy
    /// while the buddy lies inside the array and is a free block of the same
    /// order, as `__free_one_page` does (`mm/page_alloc.c:940-1005`).
    /// Every frame of the block except its head must already be `Tail`.
    fn free_block(&mut self, mut pfn: u64, mut order: u8) {
        self.free_pages += 1 << order;
        while order < MAX_PAGE_ORDER {
            // `__find_buddy_pfn` (mm/page_alloc.h:177).
            let buddy = pfn ^ (1 << order);
            let Some(b) = self.index(buddy) else { break };
            if !matches!(self.frames[b].state, FrameState::Free { order: o, .. } if o == order) {
                break;
            }
            self.unlink(b, order);
            // The higher of the two heads is merged away.
            let merged_away = pfn.max(buddy);
            if let Some(m) = self.index(merged_away) {
                self.frames[m].state = FrameState::Tail;
            }
            pfn = pfn.min(buddy);
            order += 1;
        }
        if let Some(i) = self.index(pfn) {
            self.push(i, order);
        }
    }

    /// Makes frame `i` the head of a free block and puts it first on the
    /// list of `order`.
    fn push(&mut self, i: usize, order: u8) {
        let next = self.heads[order as usize];
        if let Some(n) = next {
            self.set_prev(index_of(n), link_of(i));
        }
        self.frames[i].state = FrameState::Free { order, prev: None, next };
        self.heads[order as usize] = link_of(i);
        self.counts[order as usize] += 1;
    }

    /// Takes the free block headed by frame `i` off the list of `order`.
    fn unlink(&mut self, i: usize, order: u8) {
        let FrameState::Free { prev, next, .. } = self.frames[i].state else { return };
        match prev {
            Some(p) => self.set_next(index_of(p), next),
            None => self.heads[order as usize] = next,
        }
        if let Some(n) = next {
            self.set_prev(index_of(n), prev);
        }
        self.counts[order as usize] -= 1;
    }

    fn set_prev(&mut self, i: usize, link: Link) {
        if let FrameState::Free { prev, .. } = &mut self.frames[i].state {
            *prev = link;
        }
    }

    fn set_next(&mut self, i: usize, link: Link) {
        if let FrameState::Free { next, .. } = &mut self.frames[i].state {
            *next = link;
        }
    }
}

fn link_of(i: usize) -> Link {
    NonZeroU32::new(u32::try_from(i + 1).ok()?)
}

fn index_of(link: NonZeroU32) -> usize {
    link.get() as usize - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prandom::XorShift64;

    #[test]
    fn add_free_range_uses_largest_aligned_blocks() {
        let mut f = vec![Frame::RESERVED; 2048];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0));
        b.add_free_range(Pfn::new(1), Pfn::new(2048));
        assert_eq!(b.free_pages(), 2047);
        for o in 0..=10 {
            assert_eq!(b.free_blocks(o), 1, "order {o}");
        }
    }

    #[test]
    fn add_free_range_ignores_frames_outside_the_array_and_already_free() {
        let mut f = vec![Frame::RESERVED; 1024];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0x1000));
        b.add_free_range(Pfn::new(0), Pfn::new(0x1100));
        assert_eq!(b.free_pages(), 0x100);
        b.add_free_range(Pfn::new(0x1000), Pfn::new(0x2000));
        assert_eq!(b.free_pages(), 1024);
        assert_eq!(b.free_blocks(10), 1);
    }

    #[test]
    fn alloc_splits_and_free_merges_back() {
        let mut f = vec![Frame::RESERVED; 1024];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0x40000));
        b.add_free_range(Pfn::new(0x40000), Pfn::new(0x40400));
        let p = b.alloc(0).unwrap();
        assert_eq!(p.pfn(), Pfn::new(0x40000));
        for o in 0..10 {
            assert_eq!(b.free_blocks(o), 1);
        }
        assert_eq!(b.free_blocks(10), 0);
        b.free(p);
        assert_eq!(b.free_blocks(10), 1);
        assert_eq!(b.free_pages(), 1024);
    }

    #[test]
    fn page_block_reports_address_and_size() {
        let mut f = vec![Frame::RESERVED; 1024];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0x40000));
        b.add_free_range(Pfn::new(0x40000), Pfn::new(0x40400));
        let p = b.alloc(2).unwrap();
        assert_eq!((p.order(), p.addr(), p.size()), (2, PhysAddr::new(0x4000_0000), 0x4000));
    }

    #[test]
    fn never_merges_across_reserved_frames() {
        let mut f = vec![Frame::RESERVED; 1024];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0));
        b.add_free_range(Pfn::new(0), Pfn::new(512));
        b.add_free_range(Pfn::new(513), Pfn::new(1024));
        let mut pages = Vec::new();
        while let Some(p) = b.alloc(0) {
            assert_ne!(p.pfn(), Pfn::new(512));
            pages.push(p);
        }
        assert_eq!(pages.len(), 1023);
        for p in pages {
            b.free(p);
        }
        assert_eq!(b.free_pages(), 1023);
        assert_eq!(b.free_blocks(10), 0);
    }

    #[test]
    fn never_merges_past_the_frame_array() {
        let mut f = vec![Frame::RESERVED; 1536];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0));
        b.add_free_range(Pfn::new(0), Pfn::new(1536));
        assert_eq!(b.free_blocks(10), 1);
        assert_eq!(b.free_blocks(9), 1);
        let p = b.alloc(9).unwrap();
        b.free(p);
        assert_eq!((b.free_blocks(10), b.free_blocks(9)), (1, 1));
    }

    #[test]
    fn alloc_returns_none_when_empty_or_order_too_big() {
        let mut f = vec![Frame::RESERVED; 2048];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0));
        assert!(b.alloc(0).is_none());
        b.add_free_range(Pfn::new(0), Pfn::new(2048));
        assert!(b.alloc(11).is_none());
        assert!(b.alloc(10).is_some() && b.alloc(10).is_some());
        assert!(b.alloc(10).is_none());
    }

    #[test]
    #[should_panic(expected = "buddy: freeing a block")]
    fn free_of_block_from_another_allocator_panics() {
        let mut fa = vec![Frame::RESERVED; 16];
        let mut fb = vec![Frame::RESERVED; 16];
        let mut a = BuddyAllocator::new(&mut fa, Pfn::new(0));
        let mut b = BuddyAllocator::new(&mut fb, Pfn::new(0));
        a.add_free_range(Pfn::new(0), Pfn::new(16));
        b.add_free_range(Pfn::new(0), Pfn::new(16));
        let p = a.alloc(0).unwrap();
        b.free(p);
    }

    #[test]
    fn free_raw_checks_the_frame_state() {
        let mut f = vec![Frame::RESERVED; 16];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(0));
        b.add_free_range(Pfn::new(0), Pfn::new(16));
        let pfn = b.alloc(1).unwrap().pfn();
        assert_eq!(b.free_raw(Pfn::new(8), 0), Err(BuddyError::NotAllocated), "a free frame");
        assert_eq!(b.free_raw(pfn, 0), Err(BuddyError::NotAllocated), "the wrong order");
        assert_eq!(b.free_raw(Pfn::new(99), 0), Err(BuddyError::NotAllocated), "outside the frames");
        assert_eq!(b.free_raw(pfn, 1), Ok(()));
        assert_eq!(b.free_raw(pfn, 1), Err(BuddyError::NotAllocated), "a double free");
        assert_eq!(b.free_pages(), 16);
    }

    #[test]
    fn stress_random_alloc_free_returns_everything() {
        const N: usize = 131_072;
        const BASE: u64 = 0x40000;
        let mut f = vec![Frame::RESERVED; N];
        let mut b = BuddyAllocator::new(&mut f, Pfn::new(BASE));
        b.add_free_range(Pfn::new(BASE), Pfn::new(BASE + N as u64));
        let mut rng = XorShift64::new(1);
        let mut used = vec![false; N];
        let mut live: Vec<PageBlock> = Vec::new();
        for _ in 0..10_000 {
            if live.is_empty() || rng.below(2) == 0 {
                let order = rng.below(11) as u8;
                if let Some(p) = b.alloc(order) {
                    let first = (p.pfn().as_u64() - BASE) as usize;
                    assert_eq!(p.pfn().as_u64() % (1 << order), 0, "block not aligned to its order");
                    for u in &mut used[first..first + (1 << order)] {
                        assert!(!*u, "live blocks overlap");
                        *u = true;
                    }
                    live.push(p);
                }
            } else {
                let p = live.swap_remove(rng.below(live.len() as u64) as usize);
                let first = (p.pfn().as_u64() - BASE) as usize;
                used[first..first + (1 << p.order())].fill(false);
                b.free(p);
            }
        }
        for p in live.drain(..) {
            b.free(p);
        }
        assert_eq!(b.free_pages(), N as u64);
        assert_eq!(b.free_blocks(10), 128);
    }
}
