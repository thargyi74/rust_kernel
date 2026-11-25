// SPDX-License-Identifier: GPL-2.0-only
//! Faults on purpose, for the QEMU scenarios (feature `test-hooks`).
//! Linux counterpart: the crash tests of `drivers/misc/lkdtm/bugs.c`.

/// Runs `brk #0x123`: a synchronous exception at the current level.
pub fn breakpoint() {
    crate::arch::breakpoint()
}

/// Aims the console at 4 GiB, where QEMU `virt` with 512 MiB of RAM has
/// nothing, so the next console write takes a synchronous external abort.
/// It reads a register there; it never writes memory.
pub fn break_console() {
    crate::console::set_base_unchecked(0x1_0000_0000)
}
