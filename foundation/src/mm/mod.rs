// SPDX-License-Identifier: GPL-2.0-only
//! Physical memory, the kernel page tables and the MMU. Linux counterpart:
//! `arm64_memblock_init` (`arch/arm64/mm/init.c`) and `paging_init`
//! (`arch/arm64/mm/mmu.c`). The algorithms are in `klib::mm` and
//! `klib::arch`.

use crate::boot::BootFdt;
use crate::{arch, console};
use core::ops::Deref;
use core::sync::atomic::{AtomicBool, Ordering::Relaxed};
use klib::mm::addr::{PhysAddr, PhysRange};
use klib::mm::layout::LinearMap;
use klib::mm::map::MapError;
use klib::mm::memblock::{MAX_REGIONS, Memblock, MemblockError};

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
/// [`paging_init`] takes it, and not a plain [`Memblock`], which safe code
/// can fill with any ranges (Decision 7 of the M2 plan). It reads like a
/// `Memblock` through `Deref`.
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
