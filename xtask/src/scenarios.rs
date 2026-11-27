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

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "boot",
        features: &[],
        machine: "virt",
        expect: Expect {
            must_contain: &[
                "rust_kernel: booting on aarch64",
                "rust_kernel: CurrentEL = EL1",
                "rust_kernel: dtb at 0x",
                "version 17",
                "psci: probing for conduit method from DT.",
                "Early memory node ranges",
                "  node   0: [mem 0x0000000040000000-0x000000005fffffff]",
                "rust_kernel: MMU on",
                "rust_kernel: spinlock self-test OK",
                // K/524288K proves the 512 MiB came from the device tree.
                "Memory: ",
                "K/524288K available",
                "rust_kernel: page allocator self-test OK",
                "rust_kernel: boot OK, powering off",
            ],
            must_not_contain: &["Kernel panic", "wrong address", "early exception"],
        },
        timeout: Duration::from_secs(10),
    },
    Scenario {
        name: "panic",
        features: &["test-panic"],
        machine: "virt",
        expect: Expect {
            must_contain: &[
                "rust_kernel: booting on aarch64",
                "Kernel panic - not syncing: test-panic feature enabled",
                "---[ end Kernel panic - not syncing: test-panic feature enabled ]---",
            ],
            must_not_contain: &["boot OK"],
        },
        timeout: Duration::from_secs(10),
    },
    Scenario {
        name: "el2",
        features: &[],
        machine: "virt,virtualization=on",
        expect: Expect {
            must_contain: &[
                "rust_kernel: CurrentEL = EL2",
                "psci: probing for conduit method from DT.",
                "Kernel panic - not syncing: rust_kernel requires EL1, booted at EL2",
            ],
            must_not_contain: &["boot OK"],
        },
        timeout: Duration::from_secs(10),
    },
    Scenario {
        name: "fault",
        features: &["test-fault"],
        machine: "virt",
        expect: Expect {
            must_contain: &[
                "rust_kernel: early exception: el1h_64_sync",
                "  ESR = 0x00000000f2000123",
                "  ELR = 0x00000000400",
            ],
            must_not_contain: &["boot OK"],
        },
        timeout: Duration::from_secs(10),
    },
    Scenario {
        // The fault report itself faults (the console points at nothing).
        // A clean power-off, not a timeout, shows the re-entry is bounded.
        name: "nested-fault",
        features: &["test-nested-fault"],
        machine: "virt",
        expect: Expect {
            must_contain: &["psci: probing for conduit method from DT."],
            must_not_contain: &["boot OK"],
        },
        timeout: Duration::from_secs(10),
    },
    Scenario {
        // A read of an address no table maps, right after the MMU is on.
        // EC 0x25 (data abort at the current EL) makes ESR start 0x96.
        name: "unmapped",
        features: &["test-unmapped"],
        machine: "virt",
        expect: Expect {
            must_contain: &[
                "rust_kernel: MMU on",
                "rust_kernel: early exception: el1h_64_sync",
                "  ESR = 0x0000000096",
                "  FAR = 0xffffff0000000000",
            ],
            must_not_contain: &["boot OK"],
        },
        timeout: Duration::from_secs(10),
    },
    Scenario {
        // A SpinLock taken with the MMU off must panic: its read-modify-write
        // atomics are unreliable on Device memory (QEMU runs them anyway).
        name: "early-lock",
        features: &["test-early-lock"],
        machine: "virt",
        expect: Expect {
            must_contain: &["Kernel panic - not syncing: SpinLock used before the MMU is on"],
            must_not_contain: &["MMU on"],
        },
        timeout: Duration::from_secs(10),
    },
    Scenario {
        // The panic message itself panics. The guard must stop the second
        // panic, not recurse until the stack overflows.
        name: "recursive-panic",
        features: &["test-recursive-panic"],
        machine: "virt",
        expect: Expect {
            must_contain: &["Kernel panic - not syncing: ", "rust_kernel: recursive panic, powering off"],
            must_not_contain: &["boot OK"],
        },
        timeout: Duration::from_secs(10),
    },
];
