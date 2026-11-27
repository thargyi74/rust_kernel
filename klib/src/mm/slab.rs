// SPDX-License-Identifier: GPL-2.0-only
//! Slab size classes for `kmalloc`. Linux counterpart: `mm/slub.c` and the
//! `kmalloc` caches of `include/linux/slab.h` (ideas only).
//!
//! A slab is one page. Its header sits in the first [`HEADER_BYTES`] and is
//! never handed out; objects of one size class follow it. Code here only
//! ever gets a `&mut SlabHeader` (through [`SlabPages`]), never the page,
//! so safe code never holds a reference to memory a caller owns.
//!
//! Simplified against Linux: one partial list per class, no per-CPU slabs,
//! free slots in a bitmap instead of a free pointer inside each object, and
//! empty slabs are not given back.

use crate::mm::addr::{PAGE_SIZE, Pfn};

/// Object sizes in bytes: the first ten `kmalloc` caches
/// (`include/linux/slab.h:804-820`). Bigger requests take whole pages.
pub const SIZE_CLASSES: [usize; 10] = [8, 16, 32, 64, 96, 128, 192, 256, 512, 1024];

/// Bytes at the start of a slab page that belong to its header.
pub const HEADER_BYTES: usize = 128;

const PAGE: usize = PAGE_SIZE as usize;

/// Marks a page as a slab: "SLABHDR!".
const MAGIC: u64 = 0x5341_4c42_4844_5221;

/// The index of the smallest class `c` with `c >= max(size, 1)` and
/// `c % align == 0`. `None` means "use whole pages".
pub fn size_class(size: usize, align: usize) -> Option<usize> {
    let size = size.max(1);
    SIZE_CLASSES.iter().position(|&c| c >= size && c % align == 0)
}

/// Where the first object starts: past the header, aligned to the largest
/// power of two that divides the class size, so every slot is aligned for
/// every alignment [`size_class`] accepts for this class.
fn first_offset(class: usize) -> usize {
    let size = SIZE_CLASSES[class];
    HEADER_BYTES.next_multiple_of(1 << size.trailing_zeros())
}

/// Objects in one slab of `class`.
pub fn objects_per_slab(class: usize) -> usize {
    (PAGE - first_offset(class)) / SIZE_CLASSES[class]
}

/// Byte offset of object `slot` inside its slab page.
pub fn slot_offset(class: usize, slot: usize) -> usize {
    first_offset(class) + slot * SIZE_CLASSES[class]
}

/// The object that starts at byte `offset` of a slab of `class`.
pub fn slot_at(class: usize, offset: usize) -> Result<usize, SlabError> {
    let rel = offset.checked_sub(first_offset(class)).ok_or(SlabError::NotAnObject)?;
    let size = SIZE_CLASSES[class];
    if rel % size != 0 || rel / size >= objects_per_slab(class) {
        return Err(SlabError::NotAnObject);
    }
    Ok(rel / size)
}

#[derive(Debug, PartialEq, Eq)]
pub enum SlabError {
    /// The object is already free.
    DoubleFree,
    /// The address is not the start of an object of this slab.
    NotAnObject,
    /// The page has no slab header.
    NotASlab,
}

/// The header at the start of every slab page (Linux keeps this in
/// `struct slab`, which overlays `struct page`).
#[derive(Clone)]
pub struct SlabHeader {
    magic: u64,
    class: u8,
    in_use: u16,
    /// On its class's partial list.
    on_partial: bool,
    /// The next slab on the partial list.
    next: Option<Pfn>,
    /// Bit `s` set: slot `s` is free.
    free: [u64; 8],
}

const _: () = assert!(size_of::<SlabHeader>() <= HEADER_BYTES);

impl SlabHeader {
    /// An empty slab of `class`: every slot free, on no list.
    pub fn new(class: usize) -> Self {
        let mut free = [0u64; 8];
        for slot in 0..objects_per_slab(class) {
            free[slot / 64] |= 1 << (slot % 64);
        }
        SlabHeader { magic: MAGIC, class: class as u8, in_use: 0, on_partial: false, next: None, free }
    }

    /// Takes the lowest free slot.
    pub fn alloc_slot(&mut self) -> Option<usize> {
        let (word, bits) = self.free.iter_mut().enumerate().find(|(_, w)| **w != 0)?;
        let bit = bits.trailing_zeros() as usize;
        *bits &= !(1 << bit);
        self.in_use += 1;
        Some(word * 64 + bit)
    }

    pub fn free_slot(&mut self, slot: usize) -> Result<(), SlabError> {
        if slot >= objects_per_slab(self.class()) {
            return Err(SlabError::NotAnObject);
        }
        let (word, bit) = (slot / 64, 1 << (slot % 64));
        if self.free[word] & bit != 0 {
            return Err(SlabError::DoubleFree);
        }
        self.free[word] |= bit;
        self.in_use -= 1;
        Ok(())
    }

    pub fn is_full(&self) -> bool {
        usize::from(self.in_use) == objects_per_slab(self.class())
    }

    /// Whether this page really holds a slab header.
    pub fn is_slab(&self) -> bool {
        self.magic == MAGIC && usize::from(self.class) < SIZE_CLASSES.len()
    }

    pub fn class(&self) -> usize {
        usize::from(self.class)
    }
}

/// Where slab pages come from. `foundation` implements it over the page
/// allocator and the linear map; the tests over a `Vec`.
pub trait SlabPages {
    /// A new page for a slab, with any valid header in it. `None` when
    /// memory is out.
    fn new_slab_page(&mut self) -> Option<Pfn>;
    /// The header of slab page `pfn`.
    fn header(&mut self, pfn: Pfn) -> &mut SlabHeader;
}

/// One list of partial slabs per class (Linux: `kmem_cache_node.partial`).
pub struct Slabs {
    partial: [Option<Pfn>; SIZE_CLASSES.len()],
}

impl Default for Slabs {
    fn default() -> Self {
        Self::new()
    }
}

impl Slabs {
    pub const fn new() -> Self {
        Slabs { partial: [None; SIZE_CLASSES.len()] }
    }

    /// An object of `class`: from the first partial slab, or from a new
    /// slab page. A slab that becomes full leaves the list.
    pub fn alloc(&mut self, pages: &mut impl SlabPages, class: usize) -> Option<(Pfn, usize)> {
        let pfn = match self.partial[class] {
            Some(pfn) => pfn,
            None => {
                let pfn = pages.new_slab_page()?;
                let h = pages.header(pfn);
                *h = SlabHeader::new(class);
                h.on_partial = true;
                self.partial[class] = Some(pfn);
                pfn
            }
        };
        let h = pages.header(pfn);
        let slot = h.alloc_slot()?;
        if h.is_full() {
            self.partial[class] = h.next.take();
            h.on_partial = false;
        }
        Some((pfn, slot))
    }

    /// Frees object `slot` of slab `pfn`. A slab that was full goes back
    /// to the head of its partial list.
    pub fn free(&mut self, pages: &mut impl SlabPages, pfn: Pfn, slot: usize) -> Result<(), SlabError> {
        let h = pages.header(pfn);
        if !h.is_slab() {
            return Err(SlabError::NotASlab);
        }
        h.free_slot(slot)?;
        if !h.on_partial {
            let class = h.class();
            h.next = self.partial[class];
            h.on_partial = true;
            self.partial[class] = Some(pfn);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prandom::XorShift64;
    use std::collections::{BTreeSet, HashSet};

    /// Slab pages without memory: headers in a `Vec`, pfn = 100 + index.
    struct FakePages {
        headers: Vec<SlabHeader>,
        limit: usize,
    }

    const BASE: u64 = 100;

    impl FakePages {
        fn new(limit: usize) -> Self {
            FakePages { headers: Vec::new(), limit }
        }
    }

    impl SlabPages for FakePages {
        fn new_slab_page(&mut self) -> Option<Pfn> {
            if self.headers.len() == self.limit {
                return None;
            }
            self.headers.push(SlabHeader::new(0));
            Some(Pfn::new(BASE + self.headers.len() as u64 - 1))
        }

        fn header(&mut self, pfn: Pfn) -> &mut SlabHeader {
            &mut self.headers[(pfn.as_u64() - BASE) as usize]
        }
    }

    const C1024: usize = 9;

    #[test]
    fn size_classes_match_kmalloc() {
        let c = |s, a| size_class(s, a).map(|i| SIZE_CLASSES[i]);
        assert_eq!(
            [c(1, 1), c(9, 8), c(65, 8), c(97, 8), c(129, 8), c(1024, 8)],
            [Some(8), Some(16), Some(96), Some(128), Some(192), Some(1024)]
        );
        assert_eq!(c(1025, 8), None);
        assert_eq!(c(16, 4096), None);
        assert_eq!(c(96, 64), Some(128));
        assert_eq!(c(0, 1), Some(8));
    }

    #[test]
    fn objects_never_overlap_the_header_or_leave_the_page() {
        for i in 0..SIZE_CLASSES.len() {
            assert!(slot_offset(i, 0) >= HEADER_BYTES);
            assert!(slot_offset(i, objects_per_slab(i) - 1) + SIZE_CLASSES[i] <= 4096);
            // One more object would not fit.
            assert!(slot_offset(i, objects_per_slab(i)) + SIZE_CLASSES[i] > 4096);
            assert!(objects_per_slab(i) <= 64 * 8, "free bitmap too small for class {i}");
        }
        assert_eq!(objects_per_slab(C1024), 3);
    }

    #[test]
    fn every_slot_is_aligned_for_every_align_its_class_accepts() {
        for i in 0..SIZE_CLASSES.len() {
            let align = 1 << SIZE_CLASSES[i].trailing_zeros();
            for s in 0..objects_per_slab(i) {
                assert_eq!(slot_offset(i, s) % align, 0, "class {i} slot {s}");
            }
        }
    }

    #[test]
    fn alloc_fills_a_slab_then_takes_a_new_page() {
        let (mut pages, mut slabs) = (FakePages::new(10), Slabs::new());
        let first: Vec<_> = (0..3).map(|_| slabs.alloc(&mut pages, C1024).unwrap()).collect();
        assert_eq!(pages.headers.len(), 1);
        assert!(first.iter().all(|&(pfn, _)| pfn == first[0].0));
        assert_eq!(first.iter().map(|&(_, s)| s).collect::<HashSet<_>>().len(), 3);
        assert!(pages.headers[0].is_full());
        let (pfn, _) = slabs.alloc(&mut pages, C1024).unwrap();
        assert_eq!(pages.headers.len(), 2);
        assert_ne!(pfn, first[0].0);
    }

    #[test]
    fn alloc_without_a_new_page_is_none() {
        let (mut pages, mut slabs) = (FakePages::new(0), Slabs::new());
        assert_eq!(slabs.alloc(&mut pages, 0), None);
    }

    #[test]
    fn free_then_alloc_reuses_the_slot() {
        let (mut pages, mut slabs) = (FakePages::new(10), Slabs::new());
        let (pfn, slot) = slabs.alloc(&mut pages, 3).unwrap();
        slabs.alloc(&mut pages, 3).unwrap();
        assert_eq!(slabs.free(&mut pages, pfn, slot), Ok(()));
        assert_eq!(slabs.alloc(&mut pages, 3), Some((pfn, slot)));
        assert_eq!(pages.headers.len(), 1);
    }

    #[test]
    fn double_free_is_detected() {
        let (mut pages, mut slabs) = (FakePages::new(10), Slabs::new());
        let (pfn, slot) = slabs.alloc(&mut pages, 0).unwrap();
        assert_eq!(slabs.free(&mut pages, pfn, slot), Ok(()));
        assert_eq!(slabs.free(&mut pages, pfn, slot), Err(SlabError::DoubleFree));
    }

    #[test]
    fn free_of_a_page_that_is_not_a_slab_is_an_error() {
        let (mut pages, mut slabs) = (FakePages::new(10), Slabs::new());
        let pfn = pages.new_slab_page().unwrap();
        pages.headers[0].magic = 0;
        assert_eq!(slabs.free(&mut pages, pfn, 0), Err(SlabError::NotASlab));
    }

    #[test]
    fn free_of_a_slot_past_the_slab_is_not_an_object() {
        let (mut pages, mut slabs) = (FakePages::new(10), Slabs::new());
        let (pfn, _) = slabs.alloc(&mut pages, C1024).unwrap();
        assert_eq!(slabs.free(&mut pages, pfn, objects_per_slab(C1024)), Err(SlabError::NotAnObject));
    }

    #[test]
    fn offset_inside_header_or_between_slots_is_not_an_object() {
        for i in 0..SIZE_CLASSES.len() {
            let last = objects_per_slab(i) - 1;
            assert_eq!(slot_at(i, 0), Err(SlabError::NotAnObject));
            assert_eq!(slot_at(i, HEADER_BYTES - 1), Err(SlabError::NotAnObject));
            assert_eq!(slot_at(i, slot_offset(i, 0)), Ok(0));
            assert_eq!(slot_at(i, slot_offset(i, 0) + 1), Err(SlabError::NotAnObject));
            assert_eq!(slot_at(i, slot_offset(i, last)), Ok(last));
            assert_eq!(slot_at(i, slot_offset(i, last + 1)), Err(SlabError::NotAnObject));
            assert_eq!(slot_at(i, 4096), Err(SlabError::NotAnObject));
        }
    }

    #[test]
    fn full_slab_returns_to_partial_list_after_a_free() {
        let (mut pages, mut slabs) = (FakePages::new(10), Slabs::new());
        let full: Vec<_> = (0..3).map(|_| slabs.alloc(&mut pages, C1024).unwrap()).collect();
        let (other, _) = slabs.alloc(&mut pages, C1024).unwrap();
        let (pfn, slot) = full[1];
        assert_eq!(slabs.free(&mut pages, pfn, slot), Ok(()));
        // The freed slab is the head again, ahead of the newer one.
        assert_eq!(slabs.alloc(&mut pages, C1024), Some((pfn, slot)));
        assert_eq!(slabs.alloc(&mut pages, C1024).map(|(p, _)| p), Some(other));
        assert_eq!(pages.headers.len(), 2);
    }

    #[test]
    fn random_alloc_free_never_hands_out_a_slot_twice() {
        let (mut pages, mut slabs) = (FakePages::new(1000), Slabs::new());
        let mut rng = XorShift64::new(3);
        let mut held: Vec<(usize, Pfn, usize)> = Vec::new();
        let mut live: BTreeSet<(Pfn, usize)> = BTreeSet::new();
        for _ in 0..5_000 {
            if !held.is_empty() && rng.below(2) == 0 {
                let (_, pfn, slot) = held.swap_remove(rng.below(held.len() as u64) as usize);
                assert!(live.remove(&(pfn, slot)));
                assert_eq!(slabs.free(&mut pages, pfn, slot), Ok(()));
            } else {
                let class = rng.below(SIZE_CLASSES.len() as u64) as usize;
                let (pfn, slot) = slabs.alloc(&mut pages, class).unwrap();
                assert!(live.insert((pfn, slot)), "slot handed out twice");
                assert_eq!(pages.header(pfn).class(), class);
                assert!(slot < objects_per_slab(class));
                held.push((class, pfn, slot));
            }
        }
        for (_, pfn, slot) in held.drain(..) {
            assert_eq!(slabs.free(&mut pages, pfn, slot), Ok(()));
        }
        assert!(pages.headers.iter().all(|h| h.in_use == 0));
    }
}
