# rust_kernel

A Linux-like kernel written in pure Rust, built as a learning project. We study Linux's code line by line and its architecture, then rebuild each part in Rust. ARM64 comes first, then x86-64 and RISC-V.

- Plan and roadmap: [`docs/superpowers/plans/2026-11-25-rust-kernel.md`](docs/superpowers/plans/2026-11-25-rust-kernel.md)
- Study notes: [`docs/study/`](docs/study/)
- License: GPL-2.0-only, the same as Linux (see `LICENSE`).

## Prerequisites (macOS, Apple Silicon)

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
source "$HOME/.cargo/env"
brew install qemu
```

The exact Rust version, its components and the bare-metal target are pinned in `rust-toolchain.toml`. rustup installs them the first time you run `cargo` here.

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

Expected `cargo xtask run` output:

```
rust_kernel: booting on aarch64
rust_kernel: CurrentEL = EL1
rust_kernel: boot OK, powering off
```

## Crate map

| Crate | Builds for | `unsafe`? | Linux counterpart |
|---|---|---|---|
| `xtask` | the Mac | (host tool) | `scripts/`, `Makefile` |
| `klib` | the Mac + kernel | forbidden | `lib/` (algorithms that can be tested on the host) |
| `foundation` | kernel | **only here** | `arch/*`, the low-level parts of `mm/`, and locking |
| `kernel` | kernel | forbidden | `init/`, `kernel/`, `mm/`, `fs/`, `drivers/` |
| `vmlinux` | kernel | entry glue only | `vmlinux` (the final linked image) |

**Path rule:** Linux path `X` corresponds to our `kernel/src/X`. For example, `kernel/sched/core.c` becomes `kernel/src/kernel/sched/core.rs`. CPU-specific code goes in `foundation/src/arch/<arch>/`.
