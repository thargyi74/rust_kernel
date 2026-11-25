// SPDX-License-Identifier: GPL-2.0-only
//! From assembly to Rust: build `BootInfo` and call the kernel.

use crate::boot::BootInfo;
use crate::{console, power};
use klib::fdt::{FDT_MAGIC, FDT_V17_HEADER_SIZE, MAX_FDT_SIZE};

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

    let info = BootInfo {
        dtb_addr,
        dtb: device_tree(dtb_addr),
        image,
        current_el: super::current_el(),
    };

    // SAFETY: `foundation::entry!` in vmlinux defines `__kernel_main` with
    // exactly this signature; the link fails if it is missing.
    unsafe { __kernel_main(info) }
}

/// Turns the device-tree pointer from `x0` into a byte slice, or `&[]` if it
/// does not look like a device tree. Only the two fields needed to size the
/// slice are read here; `klib::fdt::FdtHeader::parse` checks the rest.
fn device_tree(addr: usize) -> &'static [u8] {
    // booting.rst §2: the DTB is 8-byte aligned and at most 2 MiB.
    if addr == 0 || !addr.is_multiple_of(8) {
        return &[];
    }
    let ptr = addr as *const u8;
    // SAFETY: the bootloader passes the DTB's physical address in x0 and the
    // MMU is off, so `addr` is a physical RAM address. It is non-null and
    // 8-aligned, so both aligned 32-bit reads (header bytes 0..8) are valid.
    let (magic, totalsize) = unsafe {
        let words = ptr.cast::<u32>();
        (u32::from_be(words.read()), u32::from_be(words.add(1).read()) as usize)
    };
    if magic != FDT_MAGIC || !(FDT_V17_HEADER_SIZE..=MAX_FDT_SIZE).contains(&totalsize) {
        return &[];
    }
    // SAFETY: a DTB header with the right magic says the blob is `totalsize`
    // bytes (≤ 2 MiB) starting at `addr`. Nothing writes or frees that memory
    // during boot; M2's memblock must keep it reserved to keep it 'static.
    unsafe { core::slice::from_raw_parts(ptr, totalsize) }
}
