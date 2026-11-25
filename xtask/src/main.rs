// SPDX-License-Identifier: GPL-2.0-only
//! `cargo xtask <command>`: build, run and test rust_kernel from the Mac.

mod bootlog;
mod qemu;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("test") => test(),
        _ => Err(usage()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("xtask: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    "usage: cargo xtask test".to_string()
}

/// The workspace root, so nested commands see `rust-toolchain.toml` and
/// relative paths no matter where `cargo xtask` was started.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// A `cargo` command that runs in the workspace root.
fn cargo() -> Command {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root());
    cmd
}

/// Runs `cmd` with output shown live; any non-zero exit is an error.
fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{what}: cannot start: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: failed ({status})"))
    }
}

fn test() -> Result<(), String> {
    // Plain `cargo test` at the root covers exactly the host crates listed in
    // default-members; it must keep working (Review Focus 5).
    let mut host = cargo();
    host.arg("test");
    run(host, "host tests (cargo test)")?;
    println!("xtask: all checks passed");
    Ok(())
}
