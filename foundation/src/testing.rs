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

/// Reads the virtual address `0xffff_ff00_0000_0000`. No table maps it: it
/// is in the `TTBR1` half but above the linear map. So the read takes a
/// translation fault, which the early vectors report before powering off.
pub fn read_unmapped() -> u64 {
    // SAFETY: the read faults, so it never completes and never returns
    // a value; the fault report powers the machine off. It writes nothing.
    unsafe { (0xffff_ff00_0000_0000_usize as *const u64).read_volatile() }
}
