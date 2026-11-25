// SPDX-License-Identifier: GPL-2.0-only
//! ARM PL011 UART, transmit only. Linux counterpart:
//! `drivers/tty/serial/amba-pl011.c` (`pl011_console_putchar`), registers from
//! `include/linux/amba/serial.h`.

/// QEMU `virt` puts UART0 here. Temporary: M2 reads it from the device tree.
const PL011_BASE: usize = 0x0900_0000;
/// Data register.
const UART01X_DR: usize = 0x00;
/// Flag register.
const UART01X_FR: usize = 0x18;
/// FR bit 5: transmit FIFO full.
const UART01X_FR_TXFF: u32 = 1 << 5;

/// Sends one byte, waiting while the transmit FIFO is full.
pub(crate) fn putc(byte: u8) {
    let fr = (PL011_BASE + UART01X_FR) as *const u32;
    let dr = (PL011_BASE + UART01X_DR) as *mut u32;
    // SAFETY: on QEMU `virt` a PL011 is mapped at PL011_BASE; FR and DR are
    // 32-bit device registers, accessed with aligned volatile 32-bit loads and
    // stores, and nothing else in the kernel owns this UART.
    unsafe {
        while fr.read_volatile() & UART01X_FR_TXFF != 0 {
            core::hint::spin_loop();
        }
        dr.write_volatile(u32::from(byte));
    }
}
