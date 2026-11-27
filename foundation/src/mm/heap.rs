// SPDX-License-Identifier: GPL-2.0-only
//! The kernel heap: `kmalloc` and `kfree` as Rust's `#[global_allocator]`.
//! Linux counterpart: `mm/slub.c` (`__kmalloc`, `kfree`), with big requests
//! going straight to the page allocator as `kmalloc_large` does. The size
//! classes and slab lists are `klib::mm::slab`.

use super::{MmuOn, PAGES, alloc_pages};
use crate::sync::SpinLock;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering::{Acquire, Release};
use klib::mm::addr::{PAGE_SIZE, Pfn, VirtAddr};
use klib::mm::buddy::{BuddyError, MAX_PAGE_ORDER};
use klib::mm::layout::LinearMap;
use klib::mm::slab::{SlabError, SlabHeader, SlabPages, Slabs, size_class, slot_at, slot_offset};

/// Set by [`init`]. Until then `alloc` returns null and takes no lock,
/// because the MMU may still be off. Only a plain load and store.
static READY: AtomicBool = AtomicBool::new(false);

struct Heap {
    slabs: Slabs,
    lm: LinearMap,
}

/// The heap. Lock order: this first, then `PAGES`.
static SLABS: SpinLock<Option<Heap>> = SpinLock::new(None);

/// The kernel's `#[global_allocator]`.
pub struct KernelHeap;

#[global_allocator]
static KERNEL_HEAP: KernelHeap = KernelHeap;

/// Starts the heap (Linux: `kmem_cache_init`).
///
/// # Panics
///
/// If no page allocator is installed, or on a second call.
pub(super) fn init(mmu: &MmuOn) {
    if PAGES.lock().is_none() {
        panic!("init_heap: no page allocator");
    }
    let mut heap = SLABS.lock();
    if heap.is_some() {
        panic!("init_heap called twice");
    }
    *heap = Some(Heap { slabs: Slabs::new(), lm: *mmu.linear_map() });
    READY.store(true, Release);
}

/// The smallest order with `PAGE_SIZE << order >= max(size, align)`.
/// Buddy blocks are aligned to their size, so that covers `align` too.
fn page_order(layout: Layout) -> Option<u8> {
    let need = layout.size().max(layout.align()) as u64;
    (0..=MAX_PAGE_ORDER).find(|&order| PAGE_SIZE << order >= need)
}

/// Slab pages from the page allocator, reached through the linear map.
struct HeapPages {
    lm: LinearMap,
}

impl HeapPages {
    fn header_ptr(&self, pfn: Pfn) -> *mut SlabHeader {
        self.lm.phys_to_virt(pfn.addr()).as_u64() as usize as *mut SlabHeader
    }
}

impl SlabPages for HeapPages {
    fn new_slab_page(&mut self) -> Option<Pfn> {
        // The `Pages` handle is dropped: from now on the heap owns the page,
        // and only `dealloc` (through `free_raw`) could give it back.
        let pfn = alloc_pages(0)?.addr().pfn();
        // SAFETY: the page was just allocated, so nobody else uses it. It is
        // RAM, mapped read-write in the linear map, and page-aligned, which
        // is enough for `SlabHeader`. The write makes its first bytes a
        // valid header.
        unsafe { self.header_ptr(pfn).write(SlabHeader::new(0)) };
        Some(pfn)
    }

    fn header(&mut self, pfn: Pfn) -> &mut SlabHeader {
        // SAFETY: `Slabs` passes only pfns that `new_slab_page` returned
        // (kept on its lists) or that `dealloc` passes, which come from a
        // pointer `alloc` returned for the same layout (the `GlobalAlloc`
        // contract), so `pfn` is a slab page with a valid header. Objects
        // start at `HEADER_BYTES` or later, so no caller's memory overlaps
        // the header, and `&mut self` allows one header reference at a time.
        unsafe { &mut *self.header_ptr(pfn) }
    }
}

// SAFETY: `alloc` returns null or a block of at least `layout.size()` bytes
// aligned to `layout.align()` that no other live allocation overlaps: slab
// slots are aligned for every alignment `size_class` accepts and a slot is
// handed out once until freed (klib tests), and buddy blocks are aligned to
// their size. All state is behind `SLABS` and `PAGES`, taken in that order.
unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !READY.load(Acquire) {
            return null_mut();
        }
        let mut guard = SLABS.lock();
        let Some(heap) = guard.as_mut() else { return null_mut() };
        let lm = heap.lm;
        let block = match size_class(layout.size(), layout.align()) {
            Some(class) => heap
                .slabs
                .alloc(&mut HeapPages { lm }, class)
                .map(|(pfn, slot)| lm.phys_to_virt(pfn.addr()).as_u64() + slot_offset(class, slot) as u64),
            None => page_order(layout).and_then(alloc_pages).map(|pages| lm.phys_to_virt(pages.addr()).as_u64()),
        };
        block.map_or(null_mut(), |va| va as usize as *mut u8)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let addr = ptr as usize as u64;
        let mut guard = SLABS.lock();
        let Some(heap) = guard.as_mut() else { panic!("kfree: no heap, at {:#x}", addr) };
        let lm = heap.lm;
        let Some(pa) = VirtAddr::new(addr).and_then(|va| lm.virt_to_phys(va)) else {
            panic!("kfree: {:?} at {:#x}", SlabError::NotAnObject, addr);
        };
        match size_class(layout.size(), layout.align()) {
            Some(class) => {
                let offset = (pa.as_u64() % PAGE_SIZE) as usize;
                let freed = slot_at(class, offset).and_then(|slot| heap.slabs.free(&mut HeapPages { lm }, pa.pfn(), slot));
                if let Err(e) = freed {
                    panic!("kfree: {:?} at {:#x}", e, addr);
                }
            }
            None => {
                let order = page_order(layout).filter(|_| pa.is_aligned(PAGE_SIZE));
                let freed = match (order, PAGES.lock().as_mut()) {
                    (Some(order), Some(buddy)) => buddy.free_raw(pa.pfn(), order),
                    _ => Err(BuddyError::NotAllocated),
                };
                if let Err(e) = freed {
                    panic!("kfree: {:?} at {:#x}", e, addr);
                }
            }
        }
    }
}
