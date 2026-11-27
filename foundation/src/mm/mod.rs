// SPDX-License-Identifier: GPL-2.0-only
//! Physical memory, the kernel page tables and the MMU. Linux counterpart:
//! `arm64_memblock_init` (`arch/arm64/mm/init.c`) and `paging_init`
//! (`arch/arm64/mm/mmu.c`). The algorithms are in `klib::mm` and
//! `klib::arch`.

use crate::boot::BootFdt;
use crate::sync::SpinLock;
use crate::{arch, console};
use core::ops::Deref;
use core::sync::atomic::{AtomicBool, Ordering::Relaxed};
use klib::mm::addr::{PAGE_SIZE, PhysAddr, PhysRange};
use klib::mm::buddy::{BuddyAllocator, Frame, PageBlock};
use klib::mm::layout::{ImageLayout, LinearMap};
use klib::mm::map::MapError;
use klib::mm::memblock::{MAX_REGIONS, Memblock, MemblockError};

pub mod heap;

/// Set once the boot memblock exists. Only a plain load and store, because
/// it runs with the MMU off; that is enough with one CPU and no interrupts.
/// M7 (SMP) must make this a `swap`.
static BOOT_MEMBLOCK_MADE: AtomicBool = AtomicBool::new(false);

/// The boot memblock: RAM from the boot device tree, with the kernel image,
/// the device tree and every `/memreserve/` entry reserved (Linux: the
/// global `memblock` after `arm64_memblock_init`).
///
/// Only `foundation` makes it, and only once. So the RAM it lists is real,
/// and the pages it hands out belong to nobody else. That is why
/// [`paging_init`] and [`init_page_allocator`] take it, and not a plain
/// [`Memblock`], which safe code can fill with any ranges (Decision 7 of
/// the M2 plan). It reads like a `Memblock` through `Deref`.
///
/// Limit: `/reserved-memory` nodes are not reserved yet (QEMU `virt` has
/// none). On a machine that has them, firmware memory would be handed out.
pub struct BootMemblock {
    memblock: Memblock,
    dtb: PhysRange,
}

impl BootMemblock {
    /// Fills the memblock from the boot device tree's memory nodes and
    /// reserves the image (from the linker), the device tree (where `fdt`
    /// really lies) and the `/memreserve/` entries.
    ///
    /// # Panics
    ///
    /// On a second call: two boot memblocks would hand out the same pages.
    pub fn new(fdt: &BootFdt) -> Result<BootMemblock, MemblockError> {
        if BOOT_MEMBLOCK_MADE.load(Relaxed) {
            panic!("BootMemblock::new called twice");
        }
        let layout = arch::image_layout();
        let Some(image) = PhysRange::from_bounds(layout.text.start(), layout.data.end()) else {
            panic!("BootMemblock: bad image layout {:?}", layout);
        };
        let bytes = fdt.bytes();
        let Some(dtb) = PhysRange::new(PhysAddr::new(bytes.as_ptr() as u64), bytes.len() as u64) else {
            panic!("BootMemblock: bad device tree range");
        };
        let memblock = Memblock::from_device_tree(fdt, &[image, dtb])?;
        BOOT_MEMBLOCK_MADE.store(true, Relaxed);
        Ok(BootMemblock { memblock, dtb })
    }
}

impl Deref for BootMemblock {
    type Target = Memblock;

    fn deref(&self) -> &Memblock {
        &self.memblock
    }
}

/// Proof that the MMU is on with the kernel's tables. Only [`paging_init`]
/// makes one.
pub struct MmuOn {
    lm: LinearMap,
}

impl MmuOn {
    /// The linear map of all RAM.
    pub fn linear_map(&self) -> &LinearMap {
        &self.lm
    }
}

#[derive(Debug)]
pub enum PagingError {
    /// The boot mappings could not be built.
    Map(MapError),
    /// There is no console UART to map.
    NoConsole,
    /// A known value read through the linear map came back wrong.
    LinearMapCheckFailed,
    /// The MMU is already on, so `paging_init` ran before.
    AlreadyOn,
}

/// A known value, read back through its linear alias once the MMU is on.
static CHECK: u64 = 0x5a5a_1234_5678_a5a5;

/// Whether the MMU is on.
pub fn mmu_enabled() -> bool {
    arch::mmu_enabled()
}

/// Builds the kernel page tables in pages from `memblock` and turns the
/// MMU on (Linux: `paging_init`). Afterwards the image, the device tree
/// and the console work as before, at the same addresses, and all RAM is
/// also mapped at `PAGE_OFFSET` (layout B, see `klib::mm::layout`).
pub fn paging_init(memblock: &mut BootMemblock) -> Result<MmuOn, PagingError> {
    if arch::mmu_enabled() {
        return Err(PagingError::AlreadyOn);
    }
    let (base, size) = console::mmio_range().ok_or(PagingError::NoConsole)?;
    let uart = PhysRange::new(PhysAddr::new(base as u64), size as u64).ok_or(PagingError::NoConsole)?;
    let lowest = memblock.memory().iter().map(|r| r.start()).min();
    let lm = LinearMap::new(lowest.ok_or(PagingError::Map(MapError::ImageOutsideRam))?);
    // `enable_paging` borrows the memblock mutably, so copy the RAM list.
    let mut ram = [PhysRange::EMPTY; MAX_REGIONS];
    let n = memblock.memory().len().min(MAX_REGIONS);
    for (to, from) in ram.iter_mut().zip(memblock.memory()) {
        *to = *from;
    }
    let image = arch::image_layout();
    let dtb = memblock.dtb;
    // SAFETY: the MMU is off (checked above) and M2 runs one CPU. The
    // memblock is the boot memblock: only `BootMemblock::new` makes one.
    // The image layout comes from the linker, `dtb` is where the boot
    // device tree really lies, and `uart` is the console's live window.
    unsafe { arch::enable_paging(&mut memblock.memblock, &image, dtb, uart, &ram[..n], &lm) }
        .map_err(PagingError::Map)?;

    let va = lm.phys_to_virt(PhysAddr::new(&raw const CHECK as u64));
    // SAFETY: the MMU is on and the linear map covers all RAM, which holds
    // the image (`boot_mappings` checked it), so `va` is a mapped, aligned
    // alias of `CHECK`. Reading it has no side effects.
    let seen = unsafe { (va.as_u64() as usize as *const u64).read_volatile() };
    if seen != CHECK {
        return Err(PagingError::LinearMapCheckFailed);
    }
    Ok(MmuOn { lm })
}

/// Where the kernel image lies, from the linker (for the `Memory:` line).
pub fn image_layout() -> ImageLayout {
    arch::image_layout()
}

/// The page allocator, once [`init_page_allocator`] has run (Linux: the
/// zone's `free_area`). Lock order: the heap lock first, then this one.
static PAGES: SpinLock<Option<BuddyAllocator<'static>>> = SpinLock::new(None);

/// Hands all free memory of the boot memblock to the buddy allocator
/// (Linux: `memblock_free_all` from `mm_core_init`). Inside `foundation`:
/// - the frame array is taken from the memblock in whole pages, so all of
///   it is in the linear map, and covers the lowest to the highest RAM pfn;
/// - it is filled with [`Frame::RESERVED`] through the linear map;
/// - the memblock's free ranges, without the array, go to the buddy.
///
/// No `BuddyAllocator` and no frame array come in from outside, because
/// safe code can build either over any pfns (Decision 7 of the M2 plan).
///
/// # Panics
///
/// If an allocator is already installed, or the frame array doesn't fit.
pub fn init_page_allocator(memblock: BootMemblock, mmu: &MmuOn) {
    let mut pages = PAGES.lock();
    if pages.is_some() {
        panic!("init_page_allocator: a page allocator is already installed");
    }
    let mut memblock = memblock.memblock;
    let lowest = memblock.memory().iter().map(|r| r.start()).min();
    let highest = memblock.memory().iter().map(|r| r.end()).max();
    let (Some(lowest), Some(highest)) = (lowest, highest) else {
        panic!("init_page_allocator: no memory");
    };
    let base = lowest.pfn();
    let Some(end) = highest.align_up(PAGE_SIZE) else {
        panic!("init_page_allocator: memory ends past the last page");
    };
    let count = end.pfn().as_u64() - base.as_u64();
    let bytes = count.checked_mul(size_of::<Frame>() as u64).and_then(|b| b.checked_next_multiple_of(PAGE_SIZE));
    let Some(pa) = bytes.and_then(|b| memblock.alloc(b, PAGE_SIZE)) else {
        panic!("init_page_allocator: no room for {} frames", count);
    };
    let frames = mmu.linear_map().phys_to_virt(pa).as_u64() as usize as *mut Frame;
    let count = count as usize;
    for i in 0..count {
        // SAFETY: `memblock.alloc` gave whole pages of RAM for `count`
        // frames, page-aligned (enough for `Frame`). The memblock is the
        // boot memblock, so they belong to nobody else, and `MmuOn` proves
        // the linear map covers all RAM read-write (only the image's text
        // and rodata alias is read-only, and the image is reserved).
        unsafe { frames.add(i).write(Frame::RESERVED) };
    }
    // SAFETY: the same pages, now holding `count` initialised frames. The
    // memblock is consumed here and the range stays reserved in it, so the
    // buddy never hands the pages out and nothing else can reach them: this
    // slice is their only user for the rest of the kernel's life.
    let frames: &'static mut [Frame] = unsafe { core::slice::from_raw_parts_mut(frames, count) };
    *pages = Some(memblock.into_page_allocator(frames, base));
}

/// 2^order pages from [`alloc_pages`]. Only `alloc_pages` makes one, and it
/// is neither `Clone` nor `Copy`, so a block can be freed only once, and
/// only to the allocator it came from.
pub struct Pages {
    block: PageBlock,
}

impl Pages {
    pub fn addr(&self) -> PhysAddr {
        self.block.addr()
    }

    pub fn order(&self) -> u8 {
        self.block.order()
    }

    /// Size in bytes.
    pub fn size(&self) -> u64 {
        self.block.size()
    }
}

/// 2^order contiguous pages (Linux: `alloc_pages`). `None` if no block is
/// big enough or no allocator is installed yet.
pub fn alloc_pages(order: u8) -> Option<Pages> {
    PAGES.lock().as_mut()?.alloc(order).map(|block| Pages { block })
}

/// Gives pages from [`alloc_pages`] back (Linux: `__free_pages`).
pub fn free_pages(pages: Pages) {
    match PAGES.lock().as_mut() {
        Some(buddy) => buddy.free(pages.block),
        None => panic!("free_pages: no page allocator"),
    }
}

/// Free pages in the page allocator, 0 before it exists (Linux:
/// `nr_free_pages`).
pub fn nr_free_pages() -> u64 {
    PAGES.lock().as_ref().map_or(0, |buddy| buddy.free_pages())
}

/// Starts the kernel heap, so `Vec`, `Box` and `Arc` work (Linux:
/// `kmem_cache_init`). Allocations before it fail.
///
/// # Panics
///
/// If no page allocator is installed, or on a second call.
pub fn init_heap(mmu: &MmuOn) {
    heap::init(mmu)
}
