// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `init/main.c`.

use crate::drivers::firmware::psci::psci_dt_init;
use crate::lib::locking_selftest::locking_selftest;
use crate::mm::memblock::memblock_init;
use crate::mm::mm_init::{mm_core_init, print_memory_node_ranges};
use crate::mm::selftest::{heap_selftest, page_alloc_selftest};
use crate::pr_info;
use core::ops::Range;
use foundation::boot::BootInfo;
use foundation::mm::paging_init;

/// The first generic kernel function, called once by the architecture's boot
/// code (Linux: `start_kernel()`, reached from `__primary_switched`).
pub fn start_kernel(info: BootInfo) -> ! {
    // Like Linux's `setup_machine_fdt`, a bad device tree stops the boot.
    // There is no console yet, so these two panics are silent halts.
    let fdt = info
        .device_tree()
        .unwrap_or_else(|e| panic!("no valid device tree at {:#x}: {:?}", info.dtb_addr, e));
    foundation::console::init(&fdt).unwrap_or_else(|e| panic!("no console in the device tree: {:?}", e));

    pr_info!("rust_kernel: booting on {}", foundation::arch::NAME);
    pr_info!("rust_kernel: CurrentEL = EL{}", info.current_el);
    let header = fdt.header();
    pr_info!(
        "rust_kernel: dtb at {:#x}, totalsize {} bytes, version {}",
        info.dtb_addr,
        header.totalsize,
        header.version
    );
    psci_dt_init(&fdt);
    // Before the heap exists, so the allocation fails.
    #[cfg(feature = "test-oom")]
    core::hint::black_box(alloc::boxed::Box::new(7u64));
    // After PSCI, so the guard's power-off works.
    if cfg!(feature = "test-recursive-panic") {
        panic!("{}", PanicsWhenShown);
    }
    #[cfg(feature = "test-fault")]
    foundation::testing::breakpoint();
    #[cfg(feature = "test-nested-fault")]
    {
        foundation::testing::break_console();
        foundation::testing::breakpoint();
    }

    // Linux drops from EL2 to EL1 in `init_kernel_el` (head.S); we do not yet.
    if info.current_el != 1 {
        panic!("rust_kernel requires EL1, booted at EL{}", info.current_el);
    }
    check_dtb_placement(&info);

    // `cfg!` (not `#[cfg]`) keeps the code after it reachable, so the
    // test-panic build has no "unreachable statement" warning.
    if cfg!(feature = "test-panic") {
        panic!("test-panic feature enabled");
    }

    let mut memblock = memblock_init(&fdt);
    print_memory_node_ranges(&memblock);
    #[cfg(feature = "test-early-lock")]
    drop(foundation::sync::SpinLock::new(()).lock());
    // Linux calls `paging_init` from `setup_arch`.
    let mmu = paging_init(&mut memblock).unwrap_or_else(|e| panic!("paging_init: {:?}", e));
    pr_info!("rust_kernel: MMU on");
    #[cfg(feature = "test-unmapped")]
    foundation::testing::read_unmapped();
    locking_selftest();
    // Linux: `mm_core_init`, called from `start_kernel`.
    mm_core_init(memblock, &mmu);
    page_alloc_selftest();
    heap_selftest();

    pr_info!("rust_kernel: boot OK, powering off");
    foundation::power::power_off()
}

/// Zeroing .bss or growing the stack would corrupt a device tree that lies
/// inside the kernel image, so refuse to continue if they overlap. An empty
/// `dtb` still has its address checked.
fn check_dtb_placement(info: &BootInfo) {
    let dtb: Range<usize> = info.dtb_addr..info.dtb_addr.saturating_add(info.dtb().len().max(1));
    if dtb.start < info.image.end && info.image.start < dtb.end {
        panic!(
            "device tree at {:#x} overlaps kernel image {:#x}..{:#x}",
            info.dtb_addr, info.image.start, info.image.end
        );
    }
}

/// A value whose `Display` panics, for the `recursive-panic` scenario.
struct PanicsWhenShown;

impl core::fmt::Display for PanicsWhenShown {
    fn fmt(&self, _: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        panic!("Display of PanicsWhenShown")
    }
}
