// SPDX-License-Identifier: GPL-2.0-only
//! The boot page tables, built while the MMU is off. Linux counterpart:
//! `paging_init` with `map_mem`/`map_kernel` (`arch/arm64/mm/mmu.c`), and
//! `early_pgtable_alloc`, which also takes table pages from memblock.

use klib::arch::aarch64::pgtable::{ENTRIES, Table, TableMemory, map};
use klib::mm::addr::{PAGE_SIZE, PhysAddr, PhysRange};
use klib::mm::layout::{ImageLayout, LinearMap, Root, boot_mappings};
use klib::mm::map::MapError;
use klib::mm::memblock::Memblock;

unsafe extern "C" {
    // Section boundaries from kernel.ld, all page-aligned.
    static __image_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __image_end: u8;
}

/// Where the image's sections are. The image runs identity-mapped, so a
/// symbol's address is its physical address.
pub(crate) fn image_layout() -> ImageLayout {
    // Taking the address of an extern static is safe; we never read through it.
    let range = |start: *const u8, end: *const u8| {
        PhysRange::from_bounds(PhysAddr::new(start as u64), PhysAddr::new(end as u64))
            .unwrap_or(PhysRange::EMPTY)
    };
    ImageLayout {
        text: range(&raw const __image_start, &raw const __text_end),
        rodata: range(&raw const __rodata_start, &raw const __rodata_end),
        data: range(&raw const __data_start, &raw const __image_end),
    }
}

/// Most tables the boot mappings may use. QEMU `virt` with 512 MiB needs 11.
const MAX_TABLES: usize = 64;

/// Page tables in pages taken from the boot memblock, used through their
/// physical address while the MMU is off (VA == PA). It hands out
/// references only to tables it allocated itself.
struct EarlyTables<'m> {
    memblock: &'m mut Memblock,
    owned: [PhysAddr; MAX_TABLES],
    len: usize,
}

impl EarlyTables<'_> {
    /// `pa` as a pointer, after checking it is one of our tables.
    ///
    /// # Panics
    ///
    /// If `pa` is any other address.
    fn ours(&self, pa: PhysAddr) -> *mut Table {
        let owned = self.owned.get(..self.len).unwrap_or(&[]);
        assert!(owned.contains(&pa), "EarlyTables: {:#x} is not a boot table", pa.as_u64());
        pa.as_u64() as usize as *mut Table
    }
}

impl TableMemory for EarlyTables<'_> {
    fn alloc_table(&mut self) -> Option<PhysAddr> {
        let slot = self.owned.get_mut(self.len)?;
        let pa = self.memblock.alloc(PAGE_SIZE, PAGE_SIZE)?;
        let p = pa.as_u64() as usize as *mut u64;
        for i in 0..ENTRIES {
            // SAFETY: memblock just reserved the page at `pa` for us alone.
            // It is boot-DT RAM, page-aligned, and the MMU is off, so VA ==
            // PA. Volatile keeps this a loop of aligned stores: a memset
            // could use `dc zva`, which faults while the MMU is off.
            unsafe { p.add(i).write_volatile(0) };
        }
        *slot = pa;
        self.len += 1;
        Some(pa)
    }

    fn table(&self, pa: PhysAddr) -> &Table {
        let p = self.ours(pa);
        // SAFETY: `ours` checked that `pa` is a page we allocated and zeroed
        // (reserved in memblock, so nobody else uses it). The MMU is off, so
        // the address is the pointer. The borrow is tied to `&self`, so no
        // `&mut` to it can exist at the same time.
        unsafe { &*p }
    }

    fn table_mut(&mut self, pa: PhysAddr) -> &mut Table {
        let p = self.ours(pa);
        // SAFETY: as in `table`; the borrow is tied to `&mut self`, so it is
        // the only one.
        unsafe { &mut *p }
    }
}

/// Builds the boot tables (layout B, see `klib::mm::layout`) in pages from
/// `memblock`, then turns the MMU on.
///
/// # Safety
///
/// - The MMU is off and only one CPU runs.
/// - `memblock` is the boot memblock: its memory is real RAM, and whatever
///   it still hands out belongs to nobody (the image and DTB are reserved).
/// - `image`, `dtb` and `uart` are the real image, the DTB the kernel still
///   reads and the console UART, so everything in use stays mapped.
pub(crate) unsafe fn enable_paging(
    memblock: &mut Memblock,
    image: &ImageLayout,
    dtb: PhysRange,
    uart: PhysRange,
    ram: &[PhysRange],
    lm: &LinearMap,
) -> Result<(), MapError> {
    let mut tables = EarlyTables { memblock, owned: [PhysAddr::new(0); MAX_TABLES], len: 0 };
    let low = tables.alloc_table().ok_or(MapError::OutOfTables)?;
    let high = tables.alloc_table().ok_or(MapError::OutOfTables)?;
    boot_mappings(image, dtb, uart, ram, lm, |root, m| {
        let root = match root {
            Root::Low => low,
            Root::High => high,
        };
        map(&mut tables, root, &m)
    })?;
    // SAFETY: the MMU is off (caller). `low` maps the running image (text,
    // with this code; rodata; data with the stack), the DTB and the UART,
    // and `high` maps all RAM, as `boot_mappings` emitted them, all
    // without error.
    unsafe { super::mmu::enable(low.as_u64(), high.as_u64()) };
    Ok(())
}
