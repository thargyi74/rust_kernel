// SPDX-License-Identifier: GPL-2.0-only
//! The Linux-shaped kernel proper. Linux path `X` lives at `kernel/src/X`
//! (for example `init/main.c` → `src/init/main.rs`).
//!
//! No `unsafe` is allowed here: everything that touches the machine goes
//! through the safe API of the `foundation` crate.
#![no_std]
#![forbid(unsafe_code)]

pub mod drivers;
pub mod init;
pub mod kernel;
pub mod mm;
