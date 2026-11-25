// SPDX-License-Identifier: GPL-2.0-only
//! Early exception vectors: report the exception and power off.
//! Linux counterpart: the `vectors` table (`arch/arm64/kernel/entry.S:514-529`)
//! and the register dump of `mem_abort_decode` (`arch/arm64/mm/fault.c:103`).
//!
//! Without these, a fault with the MMU work of M2 would just hang QEMU until
//! the test timed out. Each entry passes its index and the three fault
//! registers to [`early_exception`] and runs on the current stack.

use crate::console::Console;
use core::arch::{asm, global_asm};
use core::fmt::Write;
use core::sync::atomic::{AtomicU8, Ordering::Relaxed};

// 16 entries of 128 bytes in a 2 KiB-aligned table, as VBAR_EL1 requires.
global_asm!(
    ".section .text.vectors, \"ax\"",
    ".balign 2048",
    ".global early_vectors",
    "early_vectors:",
    ".irp i, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15",
    "    .balign 128",
    "    mov     x0, #\\i",
    "    mrs     x1, esr_el1",
    "    mrs     x2, far_el1",
    "    mrs     x3, elr_el1",
    "    b       early_exception",
    ".endr",
);

unsafe extern "C" {
    /// The table above.
    static early_vectors: u8;
}

/// Names in vector order, as Linux's `kernel_ventry` lines name them.
const NAMES: [&str; 16] = [
    "el1t_64_sync",
    "el1t_64_irq",
    "el1t_64_fiq",
    "el1t_64_error",
    "el1h_64_sync",
    "el1h_64_irq",
    "el1h_64_fiq",
    "el1h_64_error",
    "el0t_64_sync",
    "el0t_64_irq",
    "el0t_64_fiq",
    "el0t_64_error",
    "el0t_32_sync",
    "el0t_32_irq",
    "el0t_32_fiq",
    "el0t_32_error",
];

/// How many times `early_exception` has been entered. A fault while
/// reporting (an unmapped console, say) enters it again; without this, it
/// would recurse until the stack ran into `.bss`. Plain load and store only:
/// this runs with the MMU off.
static ENTRIES: AtomicU8 = AtomicU8::new(0);

/// Points `VBAR_EL1` at the table (Linux: `msr vbar_el1` in `head.S:225`).
pub(crate) fn install() {
    // Taking the address of an extern static is safe; we never read through it.
    let table = (&raw const early_vectors) as u64;
    // SAFETY: `early_vectors` is a complete, 2 KiB-aligned vector table in
    // the kernel image. Writing VBAR_EL1 only changes where later exceptions
    // go; the `isb` makes that take effect before the next instruction.
    // At EL2 it is harmless: EL2 exceptions use VBAR_EL2.
    unsafe { asm!("msr vbar_el1, {}", "isb", in(reg) table, options(nostack, preserves_flags)) };
}

/// Called by every vector entry. Prints the report through the lock-free
/// console, then powers off. If the report itself faults, the second entry
/// powers off at once; if powering off faults too, the third one halts.
#[unsafe(no_mangle)]
extern "C" fn early_exception(index: u64, esr: u64, far: u64, elr: u64) -> ! {
    match ENTRIES.load(Relaxed) {
        0 => ENTRIES.store(1, Relaxed),
        1 => {
            ENTRIES.store(2, Relaxed);
            crate::power::power_off()
        }
        _ => super::halt(),
    }
    let name = NAMES.get(index as usize).copied().unwrap_or("unknown");
    // The console cannot fail, so the fmt::Result carries no information.
    let _ = writeln!(Console, "rust_kernel: early exception: {name}");
    let _ = writeln!(Console, "  ESR = 0x{esr:016x}");
    let _ = writeln!(Console, "  FAR = 0x{far:016x}");
    let _ = writeln!(Console, "  ELR = 0x{elr:016x}");
    crate::power::power_off()
}
