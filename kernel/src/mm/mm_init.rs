// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `mm/mm_init.c`.

use crate::pr_info;
use foundation::mm::{BootMemblock, MmuOn, image_layout, init_heap, init_page_allocator, nr_free_pages};
use klib::mm::memblock::Memblock;

/// Prints the memory ranges, as `free_area_init` does
/// (`mm/mm_init.c:1818-1820`). There is one node, node 0.
pub fn print_memory_node_ranges(mb: &Memblock) {
    pr_info!("Early memory node ranges");
    for r in mb.memory() {
        pr_info!("  node {:3}: [mem {:#018x}-{:#018x}]", 0, r.start().as_u64(), r.end().as_u64() - 1);
    }
}

/// Gives all free memory to the page allocator, prints the `Memory:` line
/// and starts the heap (Linux: `mm_core_init` → `memblock_free_all`,
/// `mem_init_print_info` (`mm/mm_init.c:2552-2600`) and `kmem_cache_init`).
pub fn mm_core_init(memblock: BootMemblock, mmu: &MmuOn) {
    let total = memblock.total_memory() / 1024;
    init_page_allocator(memblock, mmu);
    let free = nr_free_pages() * 4;
    let image = image_layout();
    // Linux's format without the fields we don't have (init, bss, cma).
    pr_info!(
        "Memory: {}K/{}K available ({}K kernel code, {}K rwdata, {}K rodata, {}K reserved)",
        free,
        total,
        image.text.size() / 1024,
        image.data.size() / 1024,
        image.rodata.size() / 1024,
        total - free
    );
    init_heap(mmu);
}
