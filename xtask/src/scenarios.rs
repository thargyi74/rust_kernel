// SPDX-License-Identifier: GPL-2.0-only
//! The QEMU boot scenarios that `cargo xtask test` runs. Each one boots the
//! kernel, captures the serial console and checks it.

use crate::bootlog::Expect;
use std::time::Duration;

pub struct Scenario {
    pub name: &'static str,
    /// Cargo features of the `vmlinux` package to build with.
    pub features: &'static [&'static str],
    /// Value of QEMU's `-machine` option.
    pub machine: &'static str,
    pub expect: Expect,
    /// A run longer than this is a hang, and fails.
    pub timeout: Duration,
}

pub const SCENARIOS: &[Scenario] = &[Scenario {
    name: "boot",
    features: &[],
    machine: "virt",
    expect: Expect {
        must_contain: &[
            "rust_kernel: booting on aarch64",
            "rust_kernel: CurrentEL = EL1",
            "rust_kernel: dtb at 0x",
            "version 17",
            "rust_kernel: boot OK, powering off",
        ],
        must_not_contain: &["Kernel panic", "wrong address"],
    },
    timeout: Duration::from_secs(10),
}];
