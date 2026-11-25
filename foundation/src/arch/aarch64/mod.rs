// SPDX-License-Identifier: GPL-2.0-only
//! ARM64 (AArch64). Linux counterpart: `arch/arm64/`.

mod boot;
mod head;
mod psci;

use core::arch::asm;

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
