// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `mm/memblock.c`, filled the way `arm64_memblock_init`
//! (`arch/arm64/mm/init.c`) and `early_init_dt_scan_memory`
//! (`drivers/of/fdt.c`) do. The algorithm itself is `klib::mm::memblock`.

use foundation::boot::BootInfo;
use klib::fdt::Fdt;
use klib::mm::addr::{PhysAddr, PhysRange};
use klib::mm::memblock::Memblock;

/// Builds the memblock from the device tree, with the kernel image and the
/// device tree itself reserved before anything can be allocated.
pub fn memblock_init(fdt: &Fdt<'_>, info: &BootInfo) -> Memblock {
    let range = |start: usize, end: usize| {
        PhysRange::from_bounds(PhysAddr::new(start as u64), PhysAddr::new(end as u64))
            .unwrap_or_else(|| panic!("memblock: bad range {:#x}..{:#x}", start, end))
    };
    let image = range(info.image.start, info.image.end);
    let dtb = range(info.dtb_addr, info.dtb_addr.saturating_add(info.dtb().len()));
    Memblock::from_device_tree(fdt, &[image, dtb])
        .unwrap_or_else(|e| panic!("memblock: {:?} (no usable memory in the device tree?)", e))
}
