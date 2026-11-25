// SPDX-License-Identifier: GPL-2.0-only
//! The first instructions of the kernel. Linux counterpart:
//! `arch/arm64/kernel/head.S` (Image header and `primary_entry`).
//!
//! Entry state (`Documentation/arch/arm64/booting.rst` §4): MMU off,
//! interrupts masked, `x0` = physical address of the device tree.

use core::arch::global_asm;

global_asm!(
    ".section .text.head, \"ax\"",
    ".global _start",
    "_start:",
    // The 64-byte Image header, booting.rst §4. QEMU recognises it by the
    // magic and then passes the device tree in x0, as for Linux.
    "    b       primary_entry", // code0: branch over the header
    "    .long   0",             // code1
    "    .quad   0x80000",       // text_offset: load at RAM base + 0x80000
    "    .quad   __image_size",  // image_size: from kernel.ld, includes .bss and stack
    "    .quad   0x2",           // flags: little-endian, 4 KiB pages
    "    .quad   0",             // res2
    "    .quad   0",             // res3
    "    .quad   0",             // res4
    "    .long   0x644d5241",    // magic: \"ARM\\x64\"
    "    .long   0",             // res5: no PE/COFF header
    "primary_entry:",
    // Keep the DTB pointer in a callee-saved register until Rust runs.
    "    mov     x19, x0",
    "    adrp    x1, __stack_top",
    "    add     x1, x1, :lo12:__stack_top",
    "    mov     sp, x1",
    // Zero .bss 8 bytes at a time (kernel.ld aligns both ends to 16).
    // The stack lives in its own section, so this does not erase it.
    "    adrp    x1, __bss_start",
    "    add     x1, x1, :lo12:__bss_start",
    "    adrp    x2, __bss_end",
    "    add     x2, x2, :lo12:__bss_end",
    "1:  cmp     x1, x2",
    "    b.hs    2f",
    "    str     xzr, [x1], #8",
    "    b       1b",
    // __rust_boot(dtb_addr, runtime address of _start, linked address of _start)
    "2:  mov     x0, x19",
    "    adr     x1, _start",
    "    ldr     x2, =_start",
    "    bl      __rust_boot",
    "3:  wfe",
    "    b       3b",
    ".ltorg",
);
