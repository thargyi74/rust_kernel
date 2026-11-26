/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * Layout of the ARM64 kernel image.
 * Linux counterpart: arch/arm64/kernel/vmlinux.lds.S
 *
 * QEMU loads a Linux-format Image at RAM base (0x40000000) + text_offset
 * (0x80000, see head.rs). We link at exactly that address because the MMU is
 * off and nothing relocates us.
 *
 * Since M2 the image is identity-mapped with per-section permissions, so
 * .rodata, .data and the end of the image each start on a page boundary
 * (Linux: SEGMENT_ALIGN). The symbols below feed image_layout().
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
    . = ALIGN(4096);
    __text_end = .;

    __rodata_start = .;
    .rodata : { *(.rodata .rodata.*) }
    . = ALIGN(4096);
    __rodata_end = .;

    /* .data, .bss and the stack: read-write. */
    __data_start = .;
    .data : { *(.data .data.*) }

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

    . = ALIGN(4096);
    __image_end = .;
    /* Absolute symbol for the header's image_size (Linux: _kernel_size_le). */
    __image_size = __image_end - __image_start;

    /DISCARD/ : { *(.comment) *(.eh_frame*) *(.note*) }
}
