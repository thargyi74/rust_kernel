// SPDX-License-Identifier: GPL-2.0-only
//! CPU-specific code. Each architecture lives in its own folder and provides
//! the same small set of items; generic code only ever uses this module, never
//! `#[cfg(target_arch)]` (Linux: `arch/<arch>/` behind `asm/` headers).

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
mod aarch64;
#[cfg(all(target_arch = "aarch64", target_os = "none"))]
use aarch64 as imp;

#[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
compile_error!("foundation supports only bare-metal aarch64 so far (build it through `cargo xtask`)");

/// Name of the CPU architecture this kernel was built for.
pub const NAME: &str = imp::NAME;

pub(crate) use imp::{halt, system_off};
