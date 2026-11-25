// SPDX-License-Identifier: GPL-2.0-only
//! The final kernel image (Linux: `vmlinux`). It only connects foundation's
//! boot code to the kernel's first function.
#![no_std]
#![no_main]

foundation::entry!(kernel::init::main::start_kernel);
