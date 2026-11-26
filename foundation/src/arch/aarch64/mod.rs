// SPDX-License-Identifier: GPL-2.0-only
//! ARM64 (AArch64). Linux counterpart: `arch/arm64/`.

mod boot;
mod head;
mod mmu;
mod paging;
mod psci;
pub(crate) mod vectors;

use core::arch::asm;

pub(crate) use mmu::mmu_enabled;
pub(crate) use paging::{enable_paging, image_layout};
pub(crate) use psci::system_off;

pub const NAME: &str = "aarch64";

/// The exception level the CPU is running at (0–3), from `CurrentEL[3:2]`.
pub(crate) fn current_el() -> u8 {
    let el: u64;
    // SAFETY: reading CurrentEL has no side effects and touches no memory.
    unsafe { asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack, preserves_flags)) };
    ((el >> 2) & 3) as u8
}

/// Stops this CPU for good: waits for events forever (Linux: `cpu_park_loop`).
pub(crate) fn halt() -> ! {
    loop {
        // SAFETY: `wfe` only waits for an event; it touches no memory.
        unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) };
    }
}

/// A breakpoint with immediate 0x123, for the `fault` scenario.
#[cfg(feature = "test-hooks")]
pub(crate) fn breakpoint() {
    // SAFETY: `brk` raises a synchronous exception, which the early vectors
    // report before powering off; it touches no memory itself.
    unsafe { asm!("brk #0x123", options(nomem, nostack)) };
}
