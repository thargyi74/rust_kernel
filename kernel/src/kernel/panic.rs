// SPDX-License-Identifier: GPL-2.0-only
//! Kernel panic. Linux counterpart: `vpanic()` in `kernel/panic.c`, whose
//! banner lines we print word for word.

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    crate::pr_emerg!("Kernel panic - not syncing: {}", info.message());
    if let Some(location) = info.location() {
        crate::pr_emerg!("  at {}:{}", location.file(), location.line());
    }
    crate::pr_emerg!("---[ end Kernel panic - not syncing: {} ]---", info.message());
    // Linux spins or reboots here; in QEMU, powering off ends the run at once.
    foundation::power::power_off()
}
