// SPDX-License-Identifier: GPL-2.0-only
//! Machine power control.
//! Linux counterpart: `kernel_power_off` (`kernel/reboot.c:715`).

/// Powers the machine off. Never returns.
pub fn power_off() -> ! {
    crate::arch::system_off()
}
