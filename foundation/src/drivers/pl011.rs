// SPDX-License-Identifier: GPL-2.0-only
//! ARM PL011 UART, transmit only. Linux counterpart:
//! `drivers/tty/serial/amba-pl011.c` (`pl011_console_putchar`), registers from
//! `include/linux/amba/serial.h`.

/// Data register.
const UART01X_DR: usize = 0x00;
/// Flag register.
const UART01X_FR: usize = 0x18;
/// FR bit 5: transmit FIFO full.
const UART01X_FR_TXFF: u32 = 1 << 5;

/// Bytes of registers this driver touches: up to the end of FR.
pub(crate) const REGS_SIZE: usize = UART01X_FR + 4;

/// Sends one byte to the PL011 at `base`, waiting while its transmit FIFO is
/// full. `base` comes from `console::init`, which took it from the boot
/// device tree (a `BootFdt`), from a node compatible with `arm,pl011`, and
/// checked it is 4-byte aligned and its `reg` covers [`REGS_SIZE`] bytes.
pub(crate) fn putc(base: usize, byte: u8) {
    let fr = (base + UART01X_FR) as *const u32;
    let dr = (base + UART01X_DR) as *mut u32;
    // SAFETY: the bootloader's device tree says a PL011 is at `base`, with at least
    // REGS_SIZE bytes of registers (console::init checked this). FR and DR
    // are 32-bit device registers, accessed with aligned volatile 32-bit
    // loads and stores, and nothing else in the kernel owns this UART.
    unsafe {
        while fr.read_volatile() & UART01X_FR_TXFF != 0 {
            core::hint::spin_loop();
        }
        dr.write_volatile(u32::from(byte));
    }
}
