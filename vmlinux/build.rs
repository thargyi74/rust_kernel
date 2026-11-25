// SPDX-License-Identifier: GPL-2.0-only
//! Passes the architecture's linker script to the linker for kernel builds.

use std::env;
use std::path::PathBuf;

fn main() {
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if os == "none" && arch == "aarch64" {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
        let script = manifest
            .join("../foundation/src/arch/aarch64/kernel.ld")
            .canonicalize()
            .expect("foundation/src/arch/aarch64/kernel.ld must exist");
        println!("cargo:rustc-link-arg-bins=-T{}", script.display());
        println!("cargo:rerun-if-changed={}", script.display());
    }
}
