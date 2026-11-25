// SPDX-License-Identifier: GPL-2.0-only
//! PSCI firmware calls. Linux counterpart: `drivers/firmware/psci/psci.c`
//! (`psci_sys_poweroff`); function IDs from `include/uapi/linux/psci.h`.

use core::arch::asm;

/// `PSCI_0_2_FN_SYSTEM_OFF` = `PSCI_0_2_FN_BASE` (0x8400_0000) + 8.
const PSCI_0_2_FN_SYSTEM_OFF: u64 = 0x8400_0008;

/// Asks the firmware to power the machine off. In QEMU this ends the process
/// with exit status 0.
///
/// The conduit is a temporary guess from the exception level: QEMU `virt`
/// uses `hvc` when the kernel runs at EL1 and `smc` with
/// `virtualization=on` (EL2). M2 reads the `/psci` `method` from the device
/// tree, as Linux's `get_set_conduit_method` does.
pub(crate) fn system_off() -> ! {
    // SAFETY: SYSTEM_OFF takes no arguments besides the function ID in x0 and
    // accesses no kernel memory. x0–x17 may be clobbered (SMCCC), declared by
    // clobber_abi("C").
    unsafe {
        if super::current_el() == 2 {
            asm!("smc #0", inout("x0") PSCI_0_2_FN_SYSTEM_OFF => _, clobber_abi("C"), options(nostack));
        } else {
            asm!("hvc #0", inout("x0") PSCI_0_2_FN_SYSTEM_OFF => _, clobber_abi("C"), options(nostack));
        }
    }
    // Only reached if the firmware refused; a test then times out and fails.
    loop {
        core::hint::spin_loop();
    }
}
