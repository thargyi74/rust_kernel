// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `mm/mm_init.c`.

use crate::pr_info;
use klib::mm::memblock::Memblock;

/// Prints the memory ranges, as `free_area_init` does
/// (`mm/mm_init.c:1818-1820`). There is one node, node 0.
pub fn print_memory_node_ranges(mb: &Memblock) {
    pr_info!("Early memory node ranges");
    for r in mb.memory() {
        pr_info!("  node {:3}: [mem {:#018x}-{:#018x}]", 0, r.start().as_u64(), r.end().as_u64() - 1);
    }
}
