// SPDX-License-Identifier: GPL-2.0-only
//! The boot console. Like Linux's `uart_console_write`
//! (`drivers/tty/serial/serial_core.c`), every `\n` is sent as `\r\n` so a
//! serial terminal returns to column 0.
//!
//! The UART is the device-tree node named by `/chosen` `stdout-path`, as in
//! Linux's `of_console_check` / earlycon `stdout-path` handling. Until
//! [`init`] finds it, output is dropped.

use crate::drivers::pl011;
use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use klib::fdt::Fdt;

/// UART base address; 0 means no console yet. Only plain loads and stores:
/// this runs with the MMU off, where read-modify-write atomics fault.
static UART_BASE: AtomicUsize = AtomicUsize::new(0);
/// Size of the UART's `reg` window.
static UART_SIZE: AtomicUsize = AtomicUsize::new(0);

/// Why [`init`] found no console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleError {
    /// `/chosen` `stdout-path` is missing or names no node.
    NoStdoutPath,
    /// The node is not compatible with `arm,pl011`.
    NotPl011,
    /// The node's `reg` is missing, malformed, misaligned or too small.
    NoReg,
}

/// Finds the console UART in the device tree and starts using it.
pub fn init(fdt: &Fdt<'_>) -> Result<(), ConsoleError> {
    let node = fdt.chosen_stdout().ok_or(ConsoleError::NoStdoutPath)?;
    if !node.is_compatible("arm,pl011") {
        return Err(ConsoleError::NotPl011);
    }
    let (base, size) = node.reg().ok().and_then(|mut r| r.next()).ok_or(ConsoleError::NoReg)?;
    let base = usize::try_from(base).map_err(|_| ConsoleError::NoReg)?;
    let size = usize::try_from(size).map_err(|_| ConsoleError::NoReg)?;
    if base == 0 || !base.is_multiple_of(4) || size < pl011::REGS_SIZE || base.checked_add(size).is_none() {
        return Err(ConsoleError::NoReg);
    }
    UART_SIZE.store(size, Relaxed);
    UART_BASE.store(base, Relaxed);
    Ok(())
}

/// The console UART's MMIO window as `(base, size)`, once [`init`] succeeded.
pub fn mmio_range() -> Option<(usize, usize)> {
    match UART_BASE.load(Relaxed) {
        0 => None,
        base => Some((base, UART_SIZE.load(Relaxed))),
    }
}

/// Writes `s` to the console without any formatting machinery. Drops it if
/// there is no console yet.
pub fn write_str(s: &str) {
    let base = UART_BASE.load(Relaxed);
    if base == 0 {
        return;
    }
    for byte in s.bytes() {
        if byte == b'\n' {
            pl011::putc(base, b'\r');
        }
        pl011::putc(base, byte);
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
