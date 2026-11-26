// SPDX-License-Identifier: GPL-2.0-only
//! The MMU registers. Linux counterpart: the MAIR/TCR setup of
//! `__cpu_setup` (`arch/arm64/mm/proc.S:490`) and `__enable_mmu`
//! (`arch/arm64/kernel/head.S:459-473`) with its `set_sctlr_el1`
//! (`arch/arm64/include/asm/assembler.h:739-750`).

use core::arch::asm;
use klib::arch::aarch64::pgtable::{MAIR_EL1_VALUE, tcr_el1};

/// SCTLR_EL1 bits we set: M (MMU), C (data cache) and I (instruction cache).
const SCTLR_M: u64 = 1 << 0;
const SCTLR_C: u64 = 1 << 2;
const SCTLR_I: u64 = 1 << 12;

/// Whether the MMU is on (`SCTLR_EL1.M`).
pub(crate) fn mmu_enabled() -> bool {
    let sctlr: u64;
    // SAFETY: reading SCTLR_EL1 has no side effects and touches no memory.
    unsafe { asm!("mrs {}, sctlr_el1", out(reg) sctlr, options(nomem, nostack, preserves_flags)) };
    sctlr & SCTLR_M != 0
}

/// Loads MAIR, TCR and both table roots, then turns the MMU and caches on.
///
/// # Safety
///
/// The MMU must be off, and `ttbr0`/`ttbr1` must be the physical addresses
/// of complete level-0 tables that map, with the right attributes,
/// everything the CPU touches from here on: the code that is running, the
/// stack, every static and every live reference. Only one CPU may run.
pub(crate) unsafe fn enable(ttbr0: u64, ttbr1: u64) {
    let mmfr0: u64;
    // SAFETY: reading an ID register has no side effects.
    unsafe { asm!("mrs {}, id_aa64mmfr0_el1", out(reg) mmfr0, options(nomem, nostack, preserves_flags)) };
    let tcr = tcr_el1((mmfr0 & 0xf) as u8);
    // SAFETY: the caller guarantees the MMU is off and the tables map all
    // that runs next, so the instruction after `msr sctlr_el1` (identity
    // mapped) is fetched through the new tables. The sequence is Linux's:
    // barriers before the switch, a TLB flush, and an I-cache flush after.
    unsafe {
        asm!(
            "dsb sy",
            "msr mair_el1, {mair}",
            "msr tcr_el1, {tcr}",
            "isb",
            "msr ttbr0_el1, {ttbr0}",
            "msr ttbr1_el1, {ttbr1}",
            "isb",
            "tlbi vmalle1",
            "dsb nsh",
            "isb",
            // Keep the RES1 bits: read, OR in M, C and I, write back.
            "mrs {tmp}, sctlr_el1",
            "orr {tmp}, {tmp}, {bits}",
            "msr sctlr_el1, {tmp}",
            "isb",
            "ic iallu",
            "dsb nsh",
            "isb",
            mair = in(reg) MAIR_EL1_VALUE,
            tcr = in(reg) tcr,
            ttbr0 = in(reg) ttbr0,
            ttbr1 = in(reg) ttbr1,
            bits = in(reg) SCTLR_M | SCTLR_C | SCTLR_I,
            tmp = out(reg) _,
            options(nostack),
        );
    }
}
