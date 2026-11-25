// SPDX-License-Identifier: GPL-2.0-only
//! From assembly to Rust: build `BootInfo` and call the kernel.

use crate::boot::BootInfo;
use crate::{console, power};

unsafe extern "C" {
    /// First byte of the image (kernel.ld).
    static __image_start: u8;
    /// One past the last byte of the image, including .bss and the stack.
    static __image_end: u8;
}

unsafe extern "Rust" {
    /// Defined exactly once, in `vmlinux`, by `foundation::entry!`.
    fn __kernel_main(info: BootInfo) -> !;
}

/// Called from `head.rs` with a valid stack and a zeroed .bss.
#[unsafe(no_mangle)]
extern "C" fn __rust_boot(dtb_addr: usize, runtime_start: usize, linked_start: usize) -> ! {
    // Formatting uses tables of absolute addresses, which are only right if we
    // run where we were linked. Check that first, using plain writes only.
    if runtime_start != linked_start {
        console::write_str("rust_kernel: loaded at wrong address (check text_offset)\n");
        power::power_off();
    }

    // Taking the address of an extern static is safe; we never read through it.
    let image = (&raw const __image_start) as usize..(&raw const __image_end) as usize;

    let info = BootInfo { dtb_addr, dtb: &[], image, current_el: super::current_el() };

    // SAFETY: `foundation::entry!` in vmlinux defines `__kernel_main` with
    // exactly this signature; the link fails if it is missing.
    unsafe { __kernel_main(info) }
}
