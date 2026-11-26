// SPDX-License-Identifier: GPL-2.0-only
//! Kernel panic. Linux counterpart: `vpanic()` in `kernel/panic.c`, whose
//! banner lines we print word for word.

use core::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// Set by the first panic, so a panic inside the handler (say, in a
/// `Display` it prints) powers off instead of recursing until the stack
/// overflows. Linux: the `panic_cpu` check in `vpanic`.
///
/// A plain load and store, not `swap`: the handler also runs with the MMU
/// off (the `panic` and `el2` scenarios), where read-modify-write atomics
/// fault. With one CPU that is enough; M7 (SMP) must make it a `swap`.
static PANICKING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    if PANICKING.load(Relaxed) {
        foundation::console::write_str("rust_kernel: recursive panic, powering off\n");
        foundation::power::power_off();
    }
    PANICKING.store(true, Relaxed);
    crate::pr_emerg!("Kernel panic - not syncing: {}", info.message());
    if let Some(location) = info.location() {
        crate::pr_emerg!("  at {}:{}", location.file(), location.line());
    }
    crate::pr_emerg!("---[ end Kernel panic - not syncing: {} ]---", info.message());
    // Linux spins or reboots here; in QEMU, powering off ends the run at once.
    foundation::power::power_off()
}
