# rust_kernel

A Linux-like kernel written in pure Rust, built as a learning project. We study Linux's code line by line and its architecture, then rebuild each part in Rust. ARM64 comes first, then x86-64 and RISC-V.

- Plan and roadmap: [`docs/superpowers/plans/2026-11-25-rust-kernel.md`](docs/superpowers/plans/2026-11-25-rust-kernel.md)
- Study notes: [`docs/study/`](docs/study/)
- License: GPL-2.0-only, the same as Linux (see `LICENSE`).

## Status

- **M0** (toolchain, workspace, test harness): done.
- **M1** (ARM64 boot to the serial console): done.
- **M2** (memory): planned, see [`docs/superpowers/plans/2026-11-25-m2-memory.md`](docs/superpowers/plans/2026-11-25-m2-memory.md).

## Prerequisites (macOS, Apple Silicon)

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
source "$HOME/.cargo/env"
brew install qemu
```

`rust-toolchain.toml` selects Rust `stable` (tested with 1.100.0), plus the components and the bare-metal target the kernel needs. rustup installs them the first time you run `cargo` here.

The Linux source we study goes in `reference/linux`. Git ignores that folder, and it's for reading only:

```sh
git clone https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git reference/linux
git -C reference/linux checkout v7.3
```

## Build, run, test

| Command | What it does |
|---|---|
| `cargo xtask test` | Runs every check: host unit tests, then each QEMU boot scenario (a hang counts as a failure) |
| `cargo xtask run` | Builds the kernel and boots it in QEMU in your terminal (quit with Ctrl-A, then X) |
| `cargo xtask build` | Builds `target/aarch64-unknown-none-softfloat/debug/Image` (the file QEMU boots) |
| `cargo test` | Runs only the host unit tests (`xtask`, `klib`) |

`cargo xtask test` boots three scenarios:
- `boot`: a normal boot.
- `panic`: built with `--features test-panic`; it must print Linux's panic banner and power off.
- `el2`: QEMU started at EL2; the kernel must refuse with a clear panic message.

It also checks that the kernel depends on no outside crates, and it runs clippy on every kernel crate.

Expected `cargo xtask run` output:

```
rust_kernel: booting on aarch64
rust_kernel: CurrentEL = EL1
rust_kernel: dtb at 0x48000000, totalsize 1048576 bytes, version 17
rust_kernel: boot OK, powering off
```

## Crate map

| Crate | Builds for | `unsafe`? | Linux counterpart |
|---|---|---|---|
| `xtask` | the Mac | forbidden (host tool) | `scripts/`, `Makefile` |
| `klib` | the Mac + kernel | forbidden | `lib/` (algorithms that can be tested on the host), plus pure CPU data formats in `klib/src/arch/<arch>/` (page-table descriptors, MAIR/TCR values) |
| `foundation` | kernel | **only here** | `arch/*`, the low-level parts of `mm/`, and locking |
| `kernel` | kernel | forbidden | `init/`, `kernel/`, `mm/`, `fs/`, `drivers/` |
| `vmlinux` | kernel | entry glue only | `vmlinux` (the final linked image) |

**Path rule:** Linux path `X` corresponds to our `kernel/src/X`. For example, `kernel/sched/core.c` becomes `kernel/src/kernel/sched/core.rs`. CPU-specific code goes in `foundation/src/arch/<arch>/`.
