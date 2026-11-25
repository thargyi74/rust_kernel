// SPDX-License-Identifier: GPL-2.0-only
//! Machine power control.

/// Powers the machine off. Never returns.
pub fn power_off() -> ! {
    crate::arch::system_off()
}
