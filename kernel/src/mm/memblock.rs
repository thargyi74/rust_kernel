// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `mm/memblock.c`, filled the way `arm64_memblock_init`
//! (`arch/arm64/mm/init.c`) and `early_init_dt_scan_memory`
//! (`drivers/of/fdt.c`) do. The algorithm itself is `klib::mm::memblock`;
//! `foundation::mm::BootMemblock` fills it and seals it.

use foundation::boot::BootFdt;
use foundation::mm::BootMemblock;

/// Builds the memblock from the boot device tree, with the kernel image and
/// the device tree itself reserved before anything can be allocated.
pub fn memblock_init(fdt: &BootFdt) -> BootMemblock {
    BootMemblock::new(fdt).unwrap_or_else(|e| panic!("memblock: {:?} (no usable memory in the device tree?)", e))
}
