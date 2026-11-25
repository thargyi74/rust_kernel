// SPDX-License-Identifier: GPL-2.0-only
//! Kernel messages. Linux counterpart: `kernel/printk/` and the `pr_*` macros
//! in `include/linux/printk.h`. No log levels or ring buffer yet: every
//! message goes straight to the console, one line per call.

use core::fmt::{self, Write};
use foundation::console::Console;

/// Implementation of the `pr_*!` macros: prints `args` and a newline.
#[doc(hidden)]
pub fn _printk(args: fmt::Arguments<'_>) {
    // The console cannot fail, so the fmt::Result carries no information.
    let _ = Console.write_fmt(args);
    let _ = Console.write_str("\n");
}

/// Prints an informational kernel message (with a trailing newline).
#[macro_export]
macro_rules! pr_info {
    ($($arg:tt)*) => { $crate::kernel::printk::_printk(format_args!($($arg)*)) };
}

/// Prints an emergency kernel message (with a trailing newline).
#[macro_export]
macro_rules! pr_emerg {
    ($($arg:tt)*) => { $crate::kernel::printk::_printk(format_args!($($arg)*)) };
}
