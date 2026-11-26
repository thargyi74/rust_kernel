// SPDX-License-Identifier: GPL-2.0-only
//! What the kernel learns at boot, and how `vmlinux` names its entry point.
//! Linux counterpart: the boot arguments saved by `preserve_boot_args`
//! (`arch/arm64/kernel/head.S:170`) and `__fdt_pointer` (`arch/arm64/kernel/setup.c:61`).

use core::ops::{Deref, Range};
use klib::fdt::{Fdt, FdtError};

/// Facts collected by the architecture's boot code before the kernel runs.
/// Only `foundation` can make one, and the device tree bytes can't be
/// swapped, so a [`BootFdt`] always describes the real machine.
#[derive(Debug)]
pub struct BootInfo {
    /// Physical address of the device tree, as passed by the bootloader.
    pub dtb_addr: usize,
    /// The device tree bytes, or empty if the pointer did not look valid.
    pub(crate) dtb: &'static [u8],
    /// Physical address range of the kernel image, including .bss and stack.
    pub image: Range<usize>,
    /// Exception level the kernel was entered at.
    pub current_el: u8,
}

impl BootInfo {
    /// The device tree bytes, or empty if the pointer did not look valid.
    pub fn dtb(&self) -> &'static [u8] {
        self.dtb
    }

    /// Checks the device tree the bootloader passed.
    pub fn device_tree(&self) -> Result<BootFdt, FdtError> {
        Fdt::new(self.dtb).map(|fdt| BootFdt { fdt, bytes: self.dtb })
    }
}

/// The device tree the bootloader passed, and no other (Linux:
/// `initial_boot_params`, `drivers/of/fdt.c`). `foundation` functions that
/// turn device-tree values into raw device accesses take this, not a plain
/// [`Fdt`], because safe code can build an `Fdt` from any bytes. It reads
/// like an `Fdt` through `Deref`.
pub struct BootFdt {
    fdt: Fdt<'static>,
    bytes: &'static [u8],
}

impl BootFdt {
    /// The blob's bytes, where the bootloader put them.
    pub(crate) fn bytes(&self) -> &'static [u8] {
        self.bytes
    }
}

impl Deref for BootFdt {
    type Target = Fdt<'static>;

    fn deref(&self) -> &Fdt<'static> {
        &self.fdt
    }
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
