// SPDX-License-Identifier: GPL-2.0-only
//! PSCI firmware calls. Linux counterpart: `drivers/firmware/psci/psci.c`
//! (`psci_sys_poweroff`); function IDs from `include/uapi/linux/psci.h`.

use crate::power::Conduit;
use core::arch::asm;

/// `PSCI_0_2_FN_SYSTEM_OFF` = `PSCI_0_2_FN_BASE` (0x8400_0000) + 8.
const PSCI_0_2_FN_SYSTEM_OFF: u64 = 0x8400_0008;

/// Asks the firmware to power the machine off, through the conduit the
/// device tree named. In QEMU this ends the process with exit status 0.
pub(crate) fn system_off(conduit: Conduit) -> ! {
    // SAFETY: SYSTEM_OFF takes no arguments besides the function ID in x0 and
    // accesses no kernel memory. x0–x17 may be clobbered (SMCCC), declared by
    // clobber_abi("C"). The device tree said this conduit reaches PSCI.
    unsafe {
        match conduit {
            Conduit::Hvc => {
                asm!("hvc #0", inout("x0") PSCI_0_2_FN_SYSTEM_OFF => _, clobber_abi("C"), options(nostack))
            }
            Conduit::Smc => {
                asm!("smc #0", inout("x0") PSCI_0_2_FN_SYSTEM_OFF => _, clobber_abi("C"), options(nostack))
            }
        }
    }
    // Only reached if the firmware refused; a test then times out and fails.
    super::halt()
}
