// SPDX-License-Identifier: GPL-2.0-only
//! `foundation`: the only crate in rust_kernel allowed to use `unsafe`.
//!
//! It owns everything that touches the machine directly (boot entry, CPU
//! registers, device registers, firmware calls) and offers safe functions to
//! the `kernel` crate, which is compiled with `#![forbid(unsafe_code)]`.
//! Linux counterpart: `arch/*`, plus the low-level parts of `mm/` and locking.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod arch;
pub mod boot;
pub mod console;
mod drivers;
pub mod power;
