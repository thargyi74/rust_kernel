// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `init/main.c`.

use crate::pr_info;
use foundation::boot::BootInfo;

/// The first generic kernel function, called once by the architecture's boot
/// code (Linux: `start_kernel()`, reached from `__primary_switched`).
pub fn start_kernel(info: BootInfo) -> ! {
    pr_info!("rust_kernel: booting on {}", foundation::arch::NAME);
    pr_info!("rust_kernel: CurrentEL = EL{}", info.current_el);
    pr_info!("rust_kernel: boot OK, powering off");
    foundation::power::power_off()
}
