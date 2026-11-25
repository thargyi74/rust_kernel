/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * Layout of the ARM64 kernel image.
 * Linux counterpart: arch/arm64/kernel/vmlinux.lds.S
 *
 * QEMU loads a Linux-format Image at RAM base (0x40000000) + text_offset
 * (0x80000, see head.rs). We link at exactly that address because the MMU is
 * off and nothing relocates us.
 */
ENTRY(_start)

SECTIONS
{
    . = 0x40080000;
    __image_start = .;

    .text : {
        KEEP(*(.text.head))     /* Image header + primary_entry must come first */
        *(.text .text.*)
    }

    .rodata : ALIGN(8) { *(.rodata .rodata.*) }

    .data : ALIGN(8) { *(.data .data.*) }

    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    /* Boot stack: outside .bss so zeroing .bss cannot erase it. */
    .stack (NOLOAD) : ALIGN(16) {
        . += 0x10000;           /* 64 KiB */
        __stack_top = .;
    }

    __image_end = .;
    /* Absolute symbol for the header's image_size (Linux: _kernel_size_le). */
    __image_size = __image_end - __image_start;

    /DISCARD/ : { *(.comment) *(.eh_frame*) *(.note*) }
}
