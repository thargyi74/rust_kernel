// SPDX-License-Identifier: GPL-2.0-only
//! Faults on purpose, for the QEMU scenarios (feature `test-hooks`).
//! Linux counterpart: the crash tests of `drivers/misc/lkdtm/bugs.c`.

/// Runs `brk #0x123`: a synchronous exception at the current level.
pub fn breakpoint() {
    crate::arch::breakpoint()
}
