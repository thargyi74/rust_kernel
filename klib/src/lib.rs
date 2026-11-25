// SPDX-License-Identifier: GPL-2.0-only
//! Safe kernel algorithms that also build and test on the host, the
//! counterpart of Linux's `lib/`. Nothing here touches hardware.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

pub mod fdt;
pub mod mm;
pub mod prandom;
