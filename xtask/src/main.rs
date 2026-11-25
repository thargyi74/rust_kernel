// SPDX-License-Identifier: GPL-2.0-only
//! `cargo xtask <command>`: build, run and test rust_kernel from the Mac.
//! Linux counterpart: `scripts/` and the top-level `Makefile`.
#![forbid(unsafe_code)]

mod bootlog;
mod policy;
mod qemu;
mod scenarios;

use qemu::Exit;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Bare-metal target for the kernel crates (see rust-toolchain.toml).
const KERNEL_TARGET: &str = "aarch64-unknown-none-softfloat";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("build") => features_arg(&rest)
            .and_then(|f| build_image(&f))
            .map(|image| {
                println!("{}", image.display());
            }),
        Some("run") => run_interactive(rest.contains(&"--el2")),
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
    "usage: cargo xtask build [--features <list>] | run [--el2] | test".to_string()
}

/// Parses `--features a,b` into a list (empty when absent).
fn features_arg(rest: &[&str]) -> Result<Vec<String>, String> {
    match rest {
        [] => Ok(Vec::new()),
        ["--features", list] => Ok(list.split(',').map(str::to_string).collect()),
        _ => Err(usage()),
    }
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
    let status = cmd
        .status()
        .map_err(|e| format!("{what}: cannot start: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: failed ({status})"))
    }
}

/// Runs `cmd` and returns its stdout; any non-zero exit is an error.
fn capture(mut cmd: Command, what: &str) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("{what}: cannot start: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{what}: failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `llvm-objcopy` from the toolchain's `llvm-tools` component:
/// `<sysroot>/lib/rustlib/<host triple>/bin/llvm-objcopy`.
fn llvm_objcopy() -> Result<PathBuf, String> {
    let mut sysroot = Command::new("rustc");
    sysroot.current_dir(root()).args(["--print", "sysroot"]);
    let sysroot = capture(sysroot, "rustc --print sysroot")?;
    let mut version = Command::new("rustc");
    version.current_dir(root()).arg("-vV");
    let version = capture(version, "rustc -vV")?;
    let host = version
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("rustc -vV printed no host line")?;
    let tool = Path::new(sysroot.trim())
        .join("lib/rustlib")
        .join(host)
        .join("bin/llvm-objcopy");
    if tool.exists() {
        Ok(tool)
    } else {
        Err(format!(
            "{} not found (rustup component add llvm-tools)",
            tool.display()
        ))
    }
}

/// Builds the `vmlinux` ELF for the kernel target and converts it into the
/// raw `Image` that QEMU boots. Returns the path of `Image`.
fn build_image(features: &[String]) -> Result<PathBuf, String> {
    let mut build = cargo();
    build.args(["build", "-p", "vmlinux", "--target", KERNEL_TARGET]);
    if !features.is_empty() {
        build.args(["--features", &features.join(",")]);
    }
    run(build, "cargo build -p vmlinux")?;

    let out_dir = target_dir()?.join(KERNEL_TARGET).join("debug");
    let image = out_dir.join("Image");
    let mut objcopy = Command::new(llvm_objcopy()?);
    objcopy
        .arg("-O")
        .arg("binary")
        .arg(out_dir.join("vmlinux"))
        .arg(&image);
    run(objcopy, "llvm-objcopy")?;
    Ok(image)
}

/// Cargo's build directory. It honours `CARGO_TARGET_DIR` and
/// `build.target-dir`, so it is asked from `cargo metadata` instead of assumed
/// to be `./target` (otherwise a stale kernel could be booted).
fn target_dir() -> Result<PathBuf, String> {
    let mut meta = cargo();
    meta.args(["metadata", "--no-deps", "--format-version", "1"]);
    let json = capture(meta, "cargo metadata")?;
    json_string_field(&json, "target_directory")
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata printed no target_directory".to_string())
}

/// The value of the first `"key":"value"` string field in `json`, with `\"`
/// and `\\` escapes undone. Enough for `cargo metadata` without a JSON crate.
fn json_string_field(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let mut value = String::new();
    let mut chars = json[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => value.push(chars.next()?),
            c => value.push(c),
        }
    }
    None
}

/// The QEMU command line from the plan's Global Constraints.
fn qemu_command(image: &Path, machine: &str) -> Command {
    let mut cmd = Command::new("qemu-system-aarch64");
    cmd.current_dir(root())
        .args(["-machine", machine])
        .args(["-cpu", "cortex-a72", "-m", "512M", "-smp", "1"])
        .args(["-nographic", "-no-reboot", "-accel", "tcg"])
        .arg("-kernel")
        .arg(image);
    cmd
}

fn run_interactive(el2: bool) -> Result<(), String> {
    let image = build_image(&[])?;
    let machine = if el2 {
        "virt,virtualization=on"
    } else {
        "virt"
    };
    println!("xtask: starting QEMU (quit with Ctrl-A then X)");
    run(qemu_command(&image, machine), "qemu-system-aarch64")
}

fn test() -> Result<(), String> {
    // Plain `cargo test` at the root covers exactly the host crates listed in
    // default-members; it must keep working (Review Focus 5).
    let mut host = cargo();
    host.arg("test");
    run(host, "host tests (cargo test)")?;

    // No crates.io or git packages anywhere in the workspace. `cargo test`
    // above has just brought Cargo.lock up to date.
    let lock_path = root().join("Cargo.lock");
    let lock = std::fs::read_to_string(&lock_path)
        .map_err(|e| format!("cannot read {}: {e}", lock_path.display()))?;
    let outside = policy::outside_packages(&lock);
    if !outside.is_empty() {
        return Err(format!("Cargo.lock has outside packages (not allowed): {outside:?}"));
    }
    println!("xtask: policy: no outside crates");

    // Lints for every kernel crate, including clippy::undocumented_unsafe_blocks.
    let mut clippy = cargo();
    clippy.args(["clippy", "-p", "vmlinux", "-p", "foundation", "-p", "kernel", "-p", "klib"]);
    clippy.args(["--target", KERNEL_TARGET, "--", "-D", "warnings"]);
    run(clippy, "cargo clippy (kernel crates)")?;

    for s in scenarios::SCENARIOS {
        let features: Vec<String> = s.features.iter().map(|f| f.to_string()).collect();
        let image = build_image(&features)?;
        let out = qemu::run_with_timeout(qemu_command(&image, s.machine), s.timeout)
            .map_err(|e| format!("scenario {}: cannot start QEMU: {e}", s.name))?;
        if out.exit != Exit::Exited(Some(0)) {
            return Err(format!(
                "scenario {}: QEMU ended with {:?}, expected a clean power-off\n--- log ---\n{}",
                s.name, out.exit, out.output
            ));
        }
        bootlog::check(&out.output, &s.expect).map_err(|e| format!("scenario {}: {e}", s.name))?;
        println!("xtask: scenario {}: ok", s.name);
    }
    println!("xtask: all checks passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_target_directory_from_cargo_metadata() {
        let json = r#"{"packages":[],"target_directory":"/tmp/my target","version":1,"workspace_root":"/r"}"#;
        assert_eq!(json_string_field(json, "target_directory").as_deref(), Some("/tmp/my target"));
    }

    #[test]
    fn unescapes_quotes_and_backslashes() {
        let json = r#"{"target_directory":"/a\\b\"c"}"#;
        assert_eq!(json_string_field(json, "target_directory").as_deref(), Some("/a\\b\"c"));
    }

    #[test]
    fn missing_field_is_none() {
        assert_eq!(json_string_field(r#"{"version":1}"#, "target_directory"), None);
    }
}
