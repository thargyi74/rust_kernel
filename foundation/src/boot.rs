// SPDX-License-Identifier: GPL-2.0-only
//! What the kernel learns at boot, and how `vmlinux` names its entry point.
//! Linux counterpart: the boot arguments saved by `preserve_boot_args`
//! (`arch/arm64/kernel/head.S:170`) and `__fdt_pointer` (`arch/arm64/kernel/setup.c:61`).

use core::ops::Range;

/// Facts collected by the architecture's boot code before the kernel runs.
#[derive(Debug)]
pub struct BootInfo {
    /// Physical address of the device tree, as passed by the bootloader.
    pub dtb_addr: usize,
    /// The device tree bytes, or empty if the pointer did not look valid.
    pub dtb: &'static [u8],
    /// Physical address range of the kernel image, including .bss and stack.
    pub image: Range<usize>,
    /// Exception level the kernel was entered at.
    pub current_el: u8,
}

/// Declares the kernel's entry function. Use it exactly once, in `vmlinux`:
///
/// ```ignore
/// foundation::entry!(kernel::init::main::start_kernel);
/// ```
///
/// The function must have type `fn(BootInfo) -> !`.
#[macro_export]
macro_rules! entry {
    ($path:path) => {
        /// Called by `foundation` once the CPU has a stack and .bss is zeroed.
        #[unsafe(no_mangle)]
        fn __kernel_main(info: $crate::boot::BootInfo) -> ! {
            let f: fn($crate::boot::BootInfo) -> ! = $path;
            f(info)
        }
    };
}
