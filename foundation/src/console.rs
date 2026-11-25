// SPDX-License-Identifier: GPL-2.0-only
//! The boot console. Like Linux's `uart_console_write`
//! (`drivers/tty/serial/serial_core.c`), every `\n` is sent as `\r\n` so a
//! serial terminal returns to column 0.

use core::fmt;

/// Writes `s` to the console without any formatting machinery.
pub fn write_str(s: &str) {
    for byte in s.bytes() {
        if byte == b'\n' {
            crate::arch::console_putc(b'\r');
        }
        crate::arch::console_putc(byte);
    }
}

/// The console as a `core::fmt::Write` sink, for `write!`-style output.
pub struct Console;

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_str(s);
        Ok(())
    }
}
