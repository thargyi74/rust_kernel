# Linux-like Kernel in Pure Rust — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Learn how Linux works by studying its code line by line and its architecture and engineering techniques, then rebuild each part as a Linux-like kernel in Rust. ARM64 comes first, then x86-64 and RISC-V.

**Architecture:** This is a monolithic kernel whose folders mirror Linux's (`init/`, `kernel/`, `mm/`, `fs/`, `drivers/`). All `unsafe` code lives in one small `foundation` crate (CPU, boot, raw memory, devices). The `kernel` crate holds the bulk of the kernel and is compiled with `#![forbid(unsafe_code)]`. Each milestone starts by studying Linux's version of that subsystem and writing a study note. Then we build our version and test it in QEMU.

**Tech Stack:**
- Rust stable (edition 2024) with `core` and `alloc` only.
- Target `aarch64-unknown-none-softfloat`.
- QEMU `virt` machine on an Apple Silicon Mac.
- A host-side `xtask` tool, std only, for build, run and test.

**Spec:** none. The user asked for the plan first. The decisions are recorded in the **Decisions** section below, taken from the 2026-11-25 conversation.

**Scope of this plan:** this document has three parts:
- The full roadmap (M0–M9).
- Detailed tasks for **M0 (toolchain, workspace, test harness)** and **M1 (ARM64 boot to serial console)** only.
- What happens after M1: each later milestone gets its own detailed plan once its study step is finished. That design depends on what the study finds, so tasks written for it now would be guesses.

---

## Decisions

**Made by the user**
- **Purpose:** learning. Study Linux's code line by line and its architecture and engineering techniques, then rebuild it in Rust.
- **Reference:** the Linux tree is cloned to `reference/linux`, which is git-ignored and used for reading only.
- **CPUs:** ARM64 first, then x86-64 and RISC-V.
- **Structure:** Linux's folder layout behind a Rust safety wall. `unsafe` code is allowed only in `foundation` and in the `vmlinux` entry glue. Every other crate uses `#![forbid(unsafe_code)]`.
- **Dependencies:** none. Kernel crates use only `core` and `alloc`, and we write everything else ourselves.

**Proposed by Claude** (not yet explicitly confirmed by the user; any of them can change)
- **System calls:** user programs see Linux's system-call numbers and argument layout. The long-term test is running a static busybox.
- **Platform:** QEMU only. Real hardware is a non-goal for now.
- **Study notes:** one per milestone, in `docs/study/NN-topic.md`, citing `path:line` in the pinned Linux tree.
- **"Pure Rust":** no C files and no `.S` files. The unavoidable assembly lives inside Rust via `asm!` / `global_asm!`. The linker script is the only non-Rust build input.
- **Multi-core:** the design assumes multiple cores from the start, with real spinlocks from M2 on. Secondary cores start in M7.

**License (decided 2026-11-25):** **GPL-2.0-only**, the same as Linux, because we study and closely follow Linux's code.
- `LICENSE` holds the text from `reference/linux/LICENSES/preferred/GPL-2.0`.
- The workspace sets `license = "GPL-2.0-only"`.
- Every `.rs` file starts with `// SPDX-License-Identifier: GPL-2.0-only`.

## Global Constraints

- **Toolchain:** Rust `stable` in `rust-toolchain.toml`, tested with 1.100.0. It was first pinned to `1.100.0`, but on 2026-11-25 the user chose to reuse the Mac's existing `stable` and save 0.7 GB. Edition `2024`, resolver `"3"`.
- **ARM64 target:** `aarch64-unknown-none-softfloat`. The kernel never uses FP/SIMD, the same as Linux (`reference/linux/arch/arm64/Makefile`, `KBUILD_RUSTFLAGS += --target=aarch64-unknown-none-softfloat`).
- **Allowed dependencies:** `foundation`, `klib`, `kernel` and `vmlinux` may depend only on each other, through `path` dependencies. Only `xtask`, which runs on the Mac, may use std. It still uses no crates.io crates.
- **`unsafe` policy:**
  - `klib/src/lib.rs` and `kernel/src/lib.rs` start with `#![forbid(unsafe_code)]`.
  - `foundation/src/lib.rs` has `#![deny(unsafe_op_in_unsafe_fn)]` and `#![deny(clippy::undocumented_unsafe_blocks)]`.
  - Every `unsafe` block has a `// SAFETY:` comment, as in `reference/linux/Documentation/rust/coding-guidelines.rst`.
- **CPU-specific code** lives only under `foundation/src/arch/<arch>/`. Generic code never uses `#[cfg(target_arch)]`. It asks `foundation` instead, for example `foundation::arch::NAME`.
- **Host builds:** only `klib` and `xtask` build for the Mac. Conditional code for bare metal uses `cfg(target_os = "none")`, never `target_arch` alone, because the M1 Mac host is also `aarch64`.
- **Workspace config:** no global `build.target` in `.cargo/config.toml`. `xtask` passes `--target` itself.
- **Linux reference:** pinned to tag `v7.3` (commit `c03c1a14507d`, 2026-10-18). Study notes cite line numbers at that tag. Never edit, commit or push anything under `reference/`.
- **QEMU command** (built by xtask): `qemu-system-aarch64 -machine virt -cpu cortex-a72 -m 512M -smp 1 -nographic -no-reboot -accel tcg -kernel <Image>`. The EL2 scenario uses `-machine virt,virtualization=on`.
- **System-call numbers:** ARM64 and RISC-V use `reference/linux/scripts/syscall.tbl`. x86-64 uses `reference/linux/arch/x86/entry/syscalls/syscall_64.tbl`.
- **No atomics until the MMU is on (M2).** With the MMU off, all data memory is Device memory, and exclusive load/store instructions there are unreliable.

## Review Focus

1. **A hung kernel must fail the test, not hang it.** QEMU spinning forever after a fault has to time out. The test then reports `TimedOut` together with the output captured so far. Covered by Task 2: `kills_process_that_outlives_timeout` and `keeps_output_printed_before_timeout`.
2. **A panic must print Linux's message and power off.** It must not hang or reboot-loop. Covered by Task 6: the `panic` scenario.
3. **Booting at EL2** (`-machine virt,virtualization=on`) must give a clear panic message and a clean power-off, not strange behaviour. Covered by Task 6: the `el2` scenario.
4. **A corrupt or hostile device-tree header** must be rejected with a typed error, never a panic. That includes bad magic, a wrong version, a size over 2 MiB, misalignment, and offset+size that overflows `u32`. Covered by Task 3's tests.
5. **`cargo test` at the repo root on the Mac must just work.** It must not try to build bare-metal crates for macOS. Covered by Task 2, step "root cargo test".

---

## Roadmap

Every milestone follows the same loop:
1. **Study:** read the listed Linux files and write `docs/study/NN-*.md`.
2. **Plan:** write a detailed plan for the milestone.
3. **Build:** implement it with tests.
4. **Verify:** the exit test passes in `cargo xtask test`.

| # | Milestone | Linux to study (in `reference/linux`) | We build | Exit test |
|---|---|---|---|---|
| M0 | Toolchain, workspace, harness (**done**) | Top-level tree, `MAINTAINERS`, `Documentation/process/2.Process.rst`, git history | Workspace, `xtask` with a QEMU runner that times out, tree-tour note | `cargo test` and `cargo xtask test` pass on the host |
| M1 | ARM64 boot + console (**done**) | `Documentation/arch/arm64/booting.rst`, `arch/arm64/kernel/head.S`, `arch/arm64/kernel/vmlinux.lds.S`, `init/main.c` (`start_kernel`), `include/linux/amba/serial.h`, `drivers/tty/serial/serial_core.c` (`uart_console_write`), `kernel/panic.c`, `drivers/firmware/psci/psci.c`, `scripts/dtc/libfdt/fdt.c` | Image header, entry assembly, PL011 console, `pr_info!`, Linux-style panic, PSCI power-off, DTB header check | Boot, panic and EL2 scenarios all pass |
| M2 | Memory | `drivers/of/fdt.c`, `mm/memblock.c`, `arch/arm64/include/asm/pgtable-hwdef.h`, `arch/arm64/mm/mmu.c`, `mm/page_alloc.c` (buddy), `mm/slub.c` (ideas only), `kernel/locking/spinlock.c` | Full DTB walker (memory, UART, PSCI method), memblock, buddy page allocator, MMU on with kernel page tables, heap (`GlobalAlloc`), `SpinLock` | Boot log shows 512 MiB found in the DTB; page-allocator stress test; `Vec`/`Box`/`Arc` work; buddy and walker host-tested |
| M3 | Exceptions, interrupts, timer, first system calls | `arch/arm64/kernel/entry.S`, `arch/arm64/kernel/entry-common.c`, `arch/arm64/kernel/syscall.c`, `drivers/irqchip/irq-gic.c` / `irq-gic-v3.c` (whichever the DTB names), `drivers/clocksource/arm_arch_timer.c`, `scripts/syscall.tbl` | Exception vectors, fault reports, GIC, timer tick, EL0 entry, `svc` dispatch by Linux numbers (`write`=64, `exit`=93) | A Rust user program prints from EL0 and exits; ticks are counted; a user fault is reported, not fatal |
| M4 | Processes + scheduling | `include/linux/sched.h`, `arch/arm64/kernel/process.c`, `kernel/sched/core.c`, `kernel/sched/fair.c` (EEVDF), `kernel/fork.c`, `kernel/exit.c`, `fs/exec.c`, `fs/binfmt_elf.c`, `include/linux/mm_types.h`, `mm/mmap.c`, `mm/memory.c`, `arch/arm64/mm/fault.c`, `arch/arm64/mm/extable.c` | Task struct, context switch, preemptive scheduler, `clone`/`execve`/`wait4`/`exit`, ELF loader, address spaces and VMAs, page faults, safe `UserPtr` copy-to/from-user | Two processes interleave under preemption; fork+exec+wait test; ELF parser host-tested |
| M5 | Files | `include/linux/fs.h`, `fs/namei.c`, `fs/open.c`, `fs/read_write.c`, `fs/file.c`, `fs/file_table.c`, `fs/dcache.c`, `fs/inode.c`, `fs/ramfs/inode.c`, `fs/pipe.c`, `init/initramfs.c` | VFS (Linux's ops tables become Rust traits), ramfs, initramfs unpacking (cpio), fd table, pipes, `/dev/console` | User program reads a file from the initramfs and pipes data between two processes; cpio parser host-tested |
| M6 | Run busybox | `kernel/signal.c`, `arch/arm64/kernel/signal.c`, `drivers/tty/n_tty.c`, `include/uapi/asm-generic/errno-base.h` | Signals, basic TTY, the remaining system calls busybox needs (the kernel logs unimplemented ones by name from `syscall.tbl`) | Static busybox `sh` runs as `/init`; `echo`, `ls /` and `cat` work |
| M7 | Multi-core | `drivers/firmware/psci/psci.c` (`CPU_ON`), `arch/arm64/kernel/smp.c`, `include/linux/percpu-defs.h` | Start secondary CPUs, per-CPU data, IPIs, per-CPU run queues | `-smp 4` boots all CPUs; the M4–M6 tests pass under load |
| M8 | x86-64 port | `arch/x86/platform/pvh/head.S`, `arch/x86/entry/entry_64.S`, `arch/x86/entry/syscalls/syscall_64.tbl` | `foundation/src/arch/x86_64/` | The same scenario suite passes on `qemu-system-x86_64` |
| M9 | RISC-V port | `arch/riscv/kernel/head.S`, `arch/riscv/kernel/entry.S`, `arch/riscv/kernel/sbi.c` | `foundation/src/arch/riscv64/` | The same scenario suite passes on `qemu-system-riscv64` |

**Later, not planned yet:** virtio-blk with ext2, networking, booting at EL2 (dropping to EL1 as Linux's `init_kernel_el` does), RCU, and real hardware.

---

## File Structure (after M1)

```
rust_kernel/
  Cargo.toml                 workspace: members, default-members = [klib, xtask], exclude = [reference]
  rust-toolchain.toml        pinned stable version, components, target
  .cargo/config.toml         [alias] xtask = "run --package xtask --"
  .gitignore                 /reference/  /target/
  README.md                  how to build, run, test; Linux→ours path map
  docs/study/TEMPLATE.md     shape of every study note
  docs/study/00-linux-tree-tour.md
  docs/study/01-arm64-boot.md
  xtask/src/main.rs          CLI: build | run [--el2] | test
  xtask/src/qemu.rs          run a command with a hard timeout, capture output
  xtask/src/bootlog.rs       check serial output against expectations
  xtask/src/scenarios.rs     boot / panic / el2 scenario table
  xtask/src/policy.rs        "no external crates" check
  klib/src/lib.rs            safe, host-testable algorithms        ↔ Linux lib/
  klib/src/fdt.rs            device-tree header check               ↔ scripts/dtc/libfdt/fdt.c
  foundation/src/lib.rs      the ONLY crate with unsafe
  foundation/src/boot.rs     BootInfo, entry! macro
  foundation/src/console.rs  Console (fmt::Write over the UART)
  foundation/src/power.rs    power_off()
  foundation/src/arch/mod.rs selects arch, exports NAME
  foundation/src/arch/aarch64/{mod.rs, head.rs, boot.rs, pl011.rs, psci.rs, kernel.ld}
                             ↔ arch/arm64/kernel/head.S, vmlinux.lds.S, amba-pl011, psci
  kernel/src/lib.rs          #![forbid(unsafe_code)]; Linux path X ↔ kernel/src/X
  kernel/src/init/main.rs    start_kernel                           ↔ init/main.c
  kernel/src/kernel/printk.rs pr_info!, pr_emerg!                   ↔ kernel/printk/
  kernel/src/kernel/panic.rs #[panic_handler]                       ↔ kernel/panic.c
  vmlinux/src/main.rs        #![no_main]; foundation::entry!(...)   ↔ vmlinux
  vmlinux/build.rs           passes -T kernel.ld to the linker
```

Build output: `target/aarch64-unknown-none-softfloat/debug/vmlinux` (ELF) and `.../debug/Image` (raw binary, the file QEMU boots).

---

## M0 — Toolchain, workspace, harness

### Task 1: Pin the Linux reference and write the tree tour

**Files:**
- Create: `docs/study/TEMPLATE.md`, `docs/study/00-linux-tree-tour.md`
- Touch (not committed): `reference/linux` checkout

**Interfaces:**
- Produces: the study-note template that every later note follows, with these headings:
  - `Linux reference` (tag + commit)
  - `Files read` (`path:line-range`)
  - `How Linux does it` (call flow, in order)
  - `Key data structures`
  - `Engineering techniques worth stealing`
  - `Our Rust design and why it differs`
  - `Open questions`

- [x] **Step 1: Pin the reference tree to v7.3**

Run: `git -C reference/linux checkout -f v7.3`

Expected: `HEAD is now at c03c1a14507d`.
- `-f` is safe here. The only "modified" files are about 13 netfilter headers whose names differ only in case (for example `xt_CONNMARK.h` and `xt_connmark.h`). macOS APFS is case-insensitive, so it can't hold both versions. They are not real edits.

- [x] **Step 2: Verify the pin**

Run: `git -C reference/linux describe --tags`. Expected: `v7.3`.

- [x] **Step 3: Write `docs/study/TEMPLATE.md`** with the headings listed under Interfaces above.

- [x] **Step 4: Write `docs/study/00-linux-tree-tour.md`.** Every number in it comes from these commands, run at v7.3. Paste each command next to its result so the note can be reproduced:
  - Size per top-level directory: `git ls-files -z -- <dir> | grep -zE '\.(c|h|S|rs)$' | xargs -0 cat | wc -l`. At the time of writing, `drivers` was about 26.8M lines out of about 38.9M total.
  - Total commits: `git rev-list --count HEAD`.
  - The first commit: `git log --reverse --format='%h %ad %s' --date=short | head -1`. Expected: `1da177e4c3f4 2005-04-16 Linux-2.6.12-rc2`. Git history starts at that import, so earlier history isn't in this repo.
  - How a release is assembled: `git log --first-parent --merges v7.2..v7.3 --format=%s | head -30`. These are the "Merge tag '…' of git://…" pulls from subsystem maintainers.
  - The release rhythm: the dates of `v7.3-rc1` through `v7.3` (`git log -1 --format=%ad <tag>`). Together they show the two-week merge window followed by the weekly `-rc` releases.
  - Who maintains what: one `MAINTAINERS` entry (`ARM64 PORT`), with its fields explained.
  - A paragraph on each top-level directory we will study: `arch init kernel mm fs drivers include lib`. Explain why we skip most of `drivers/`, `sound/` and `net/`.

- [x] **Step 5: Commit**

```bash
git add docs/study/TEMPLATE.md docs/study/00-linux-tree-tour.md
git commit -m "docs(study): Linux tree tour at v7.3 and study-note template"
```

### Task 2: Toolchain, workspace, and the QEMU test harness

**Files:**
- Create: `rust-toolchain.toml`, `Cargo.toml`, `.cargo/config.toml`, `README.md`, `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/src/qemu.rs`, `xtask/src/bootlog.rs`
- Modify: `.gitignore` (add `/target/`)

**Interfaces:**
- Produces:
  - `qemu::run_with_timeout(cmd: std::process::Command, timeout: std::time::Duration) -> std::io::Result<RunOutcome>`
  - `pub struct RunOutcome { pub output: String, pub exit: Exit }`. `output` holds stdout and stderr together.
  - `#[derive(Debug, PartialEq, Eq)] pub enum Exit { Exited(Option<i32>), TimedOut }`
  - `bootlog::Expect { pub must_contain: &'static [&'static str], pub must_not_contain: &'static [&'static str] }`
  - `bootlog::check(log: &str, expect: &Expect) -> Result<(), String>`. It turns `\r\n` into `\n` first. The `must_contain` strings must appear in order.
  - The CLI subcommand `cargo xtask test`. For now it runs plain `cargo test` at the workspace root, which covers the default members, so Review Focus 5 is checked on every run. Later tasks add more steps.

- [x] **Step 1: Install the tools.** This installs software on the Mac, so get the user's OK first.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
source "$HOME/.cargo/env"
rustup component add clippy rustfmt llvm-tools
rustup target add aarch64-unknown-none-softfloat
brew install qemu
```

- [x] **Step 2: Record the versions**

Run: `rustc --version && qemu-system-aarch64 --version | head -1`

Expected: both print versions. The Rust version must be ≥ 1.85, because edition 2024 needs it.

- [x] **Step 3: Write `rust-toolchain.toml`.** Use `channel = "<exact X.Y.Z from Step 2>"`, `components = ["clippy", "rustfmt", "llvm-tools"]`, `targets = ["aarch64-unknown-none-softfloat"]`, `profile = "minimal"`.

- [x] **Step 4: Write the workspace.**
  - `Cargo.toml`: `[workspace] resolver = "3"`, `members = ["xtask"]`, `default-members = ["xtask"]`, `exclude = ["reference"]`, and `[workspace.package] edition = "2024"`, `version = "0.1.0"`.
  - `.cargo/config.toml`: only the alias `xtask = "run --package xtask --"`.
  - `xtask/Cargo.toml`: no dependencies.

- [x] **Step 5: Write the failing tests in `xtask/src/qemu.rs`**

```rust
#[test]
fn returns_output_and_exit_code() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "echo hi; echo oops >&2; exit 3"]);
    let out = run_with_timeout(cmd, Duration::from_secs(5)).unwrap();
    assert!(out.output.contains("hi\n") && out.output.contains("oops\n"));
    assert_eq!(out.exit, Exit::Exited(Some(3)));
}

#[test]
fn kills_process_that_outlives_timeout() {
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let start = Instant::now();
    let out = run_with_timeout(cmd, Duration::from_millis(300)).unwrap();
    assert_eq!(out.exit, Exit::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn keeps_output_printed_before_timeout() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "echo partial; exec sleep 30"]); // exec: kill() must reach the process holding the pipe
    let out = run_with_timeout(cmd, Duration::from_millis(500)).unwrap();
    assert_eq!(out.exit, Exit::TimedOut);
    assert_eq!(out.output, "partial\n");
}
```

- [x] **Step 6: Write the failing tests in `xtask/src/bootlog.rs`**

```rust
const E: Expect = Expect { must_contain: &["A", "B"], must_not_contain: &["Kernel panic"] };

#[test] fn accepts_log_with_all_lines() { assert!(check("x\r\nA\r\nB\r\n", &E).is_ok()); }
#[test] fn reports_missing_line() {
    assert!(check("A\n", &E).unwrap_err().contains("missing (in order): \"B\""));
}
#[test] fn enforces_order() {
    assert!(check("B\nA\n", &E).unwrap_err().contains("missing (in order): \"B\""));
}
#[test] fn rejects_forbidden_text() {
    let err = check("A\nB\nKernel panic - not syncing: x\n", &E).unwrap_err();
    assert!(err.contains("forbidden: \"Kernel panic\""));
}
```

- [x] **Step 7: Run the tests and confirm they fail**

Run: `cargo test -p xtask`

Expected: compile errors, because `run_with_timeout`, `RunOutcome`, `Exit`, `Expect` and `check` don't exist yet.

- [x] **Step 8: Implement `run_with_timeout` and `check`.**
  - `run_with_timeout`:
    - Use `Stdio::null()` for stdin and `Stdio::piped()` for stdout and stderr.
    - Start one reader thread per pipe, each appending to an `Arc<Mutex<String>>`.
    - Poll `try_wait()` every 20 ms.
    - On timeout, `kill()` then `wait()`, join the readers, and return `TimedOut`.
  - `check`: look at `must_contain` first, then `must_not_contain`, and report the first problem only. The `Err` string ends with `\n--- log ---\n<log>`.
  - `main.rs`: `cargo xtask test` runs `cargo test` at the workspace root as a child process. Any non-zero exit makes xtask exit 1.
  - Every nested `cargo` or QEMU command xtask starts sets `current_dir` to the workspace root (`concat!(env!("CARGO_MANIFEST_DIR"), "/..")`). That keeps `rust-toolchain.toml` and the relative paths valid when xtask is run from a subfolder.

- [x] **Step 9: Run the tests and confirm they pass**

Run: `cargo test -p xtask`. Expected: 7 passed.

- [x] **Step 10: Root cargo test** (Review Focus 5)

Run: `cargo test` from the repo root. Expected: it passes and builds only the default members.

- [x] **Step 11: Write `README.md`.** Include:
  - The prerequisites from Step 1.
  - `cargo xtask test`.
  - The crate map from "File Structure".
  - The rule "Linux path X ↔ `kernel/src/X`".

- [x] **Step 12: Commit**

```bash
git add .gitignore rust-toolchain.toml Cargo.toml .cargo/config.toml README.md xtask
git commit -m "build: workspace, pinned toolchain, xtask QEMU runner with timeout"
```

### Task 3: `klib` device-tree header check (host TDD)

**Files:**
- Create: `klib/Cargo.toml`, `klib/src/lib.rs`, `klib/src/fdt.rs`
- Modify: `Cargo.toml` (add `klib` to `members` and `default-members`, so the root `cargo test` that `cargo xtask test` runs picks it up)

**Interfaces:**
- Produces, in `klib::fdt`:
  - `pub const FDT_MAGIC: u32 = 0xd00d_feed;`
  - `pub const FDT_V17_HEADER_SIZE: usize = 40;`
  - `pub const MAX_FDT_SIZE: usize = 2 * 1024 * 1024;`. This is `MAX_FDT_SIZE` in `arch/arm64/include/asm/boot.h`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct FdtHeader { pub totalsize, pub off_dt_struct, pub off_dt_strings, pub off_mem_rsvmap, pub version, pub last_comp_version, pub boot_cpuid_phys, pub size_dt_strings, pub size_dt_struct }`, all `u32`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum FdtError { Truncated, BadMagic(u32), BadVersion { version: u32, last_comp_version: u32 }, TooLarge(u32), Alignment }`
  - `impl FdtHeader { pub fn parse(blob: &[u8]) -> Result<FdtHeader, FdtError> }`
- `klib/src/lib.rs`: `#![cfg_attr(not(test), no_std)]`, `#![forbid(unsafe_code)]`, `pub mod fdt;`

**The rules, mirroring `fdt_check_header` in `scripts/dtc/libfdt/fdt.c`, checked in this order.** All fields are big-endian `u32`.
1. If `blob.len() < 40` → `Truncated`.
2. If `magic != FDT_MAGIC` → `BadMagic(magic)`.
3. If `version < 17`, or `last_comp_version > 17`, or `version < last_comp_version` → `BadVersion`.
   - Our call: Linux accepts versions from 2 upward, but we accept version 17 only. QEMU and every modern `dtc` produce version 17. Record this difference in the study note.
4. If `totalsize < 40` → `Truncated`.
5. If `totalsize > MAX_FDT_SIZE` → `TooLarge(totalsize)`.
6. If `totalsize > blob.len()` → `Truncated`.
7. If `off_mem_rsvmap % 8 != 0` or `off_dt_struct % 4 != 0` → `Alignment`.
8. If `off_mem_rsvmap` is not in `40..=totalsize` → `Truncated`.
9. Both the struct block `[off_dt_struct, off_dt_struct + size_dt_struct)` and the strings block must lie inside `[40, totalsize]`. Use `checked_add`; overflow → `Truncated`.

- [x] **Step 1: Write the failing tests in `klib/src/fdt.rs`.**
  - Use a helper `fn blob(f: [u32; 10], len: usize) -> Vec<u8>` that writes the ten fields big-endian and zero-pads to `len`.
  - The valid baseline `OK` is `[FDT_MAGIC, 256, 56, 200, 40, 17, 16, 0, 56, 144]`, with `len = 256`.

```rust
#[test] fn parses_valid_v17_header() {
    let h = FdtHeader::parse(&blob(OK, 256)).unwrap();
    assert_eq!((h.version, h.totalsize, h.size_dt_struct), (17, 256, 144));
}
#[test] fn rejects_short_blob() { assert_eq!(FdtHeader::parse(&[0u8; 39]), Err(FdtError::Truncated)); }
#[test] fn rejects_bad_magic() { let mut f = OK; f[0] = 0x1234_5678;
    assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::BadMagic(0x1234_5678))); }
#[test] fn rejects_old_version() { let mut f = OK; f[5] = 16; f[6] = 16;
    assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::BadVersion { version: 16, last_comp_version: 16 })); }
#[test] fn rejects_totalsize_beyond_blob() { let mut f = OK; f[1] = 512;
    assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated)); }
#[test] fn rejects_totalsize_over_2mib() { let mut f = OK; f[1] = 2 * 1024 * 1024 + 8;
    assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::TooLarge(2 * 1024 * 1024 + 8))); }
#[test] fn rejects_misaligned_rsvmap() { let mut f = OK; f[4] = 44;
    assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Alignment)); }
#[test] fn rejects_struct_block_past_end() { let mut f = OK; f[9] = 1000;
    assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated)); }
#[test] fn rejects_overflowing_strings_block_without_panicking() { let mut f = OK; f[3] = 0xFFFF_FFF0; f[8] = 0x20;
    assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated)); }
```

- [x] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p klib`. Expected: compile errors, because `FdtHeader` and `FdtError` aren't defined.

- [x] **Step 3: Implement `FdtHeader::parse`,** following the rules above in that order.

- [x] **Step 4: Run the tests and confirm they pass**

Run: `cargo xtask test`. Expected: `klib` 9 passed and `xtask` 7 passed.

- [x] **Step 5: Commit**

```bash
git add Cargo.toml klib
git commit -m "feat(klib): device-tree header check mirroring libfdt fdt_check_header"
```

---

## M1 — ARM64 boot to serial console

### Task 4: Study the ARM64 boot, then boot to "hello" and power off

**Files:**
- Create: `docs/study/01-arm64-boot.md`
- Create: `foundation/{Cargo.toml, src/lib.rs, src/boot.rs, src/console.rs, src/power.rs, src/arch/mod.rs}`
- Create: `foundation/src/arch/aarch64/{mod.rs, head.rs, boot.rs, pl011.rs, psci.rs, kernel.ld}`
- Create: `kernel/{Cargo.toml, src/lib.rs, src/init/mod.rs, src/init/main.rs, src/kernel/mod.rs, src/kernel/printk.rs, src/kernel/panic.rs}`
- Create: `vmlinux/{Cargo.toml, build.rs, src/main.rs}`, `xtask/src/scenarios.rs`
- Modify: `Cargo.toml` (members += `foundation`, `kernel`, `vmlinux`; NOT in default-members), `xtask/src/main.rs`, `README.md`

**Interfaces:**
- Produces:
  - `foundation::boot::BootInfo { pub dtb_addr: usize, pub dtb: &'static [u8], pub image: core::ops::Range<usize>, pub current_el: u8 }`. In this task `dtb` is `&[]`; Task 5 fills it.
  - `foundation::entry!(path)`. This macro expands in `vmlinux` to `#[unsafe(no_mangle)] fn __kernel_main(info: BootInfo) -> !`, which calls `path`. It type-checks `path` as `fn(BootInfo) -> !`.
  - `foundation::console::Console`, a unit struct that implements `core::fmt::Write`. It turns `\n` into `\r\n`, as `uart_console_write` does.
  - `foundation::power::power_off() -> !`
  - `foundation::arch::NAME: &str = "aarch64"`
  - `kernel::init::main::start_kernel(info: BootInfo) -> !`
  - `kernel::pr_info!` and `kernel::pr_emerg!`. Both format their arguments, write them to `Console` and add `\n`.
  - The `cargo xtask build [--features <f>]` subcommand. It runs `cargo build -p vmlinux --target aarch64-unknown-none-softfloat [--features <f>]`, then `llvm-objcopy -O binary vmlinux Image`. The `llvm-objcopy` path is `<rustc --print sysroot>/lib/rustlib/<host triple from rustc -vV>/bin/llvm-objcopy`.
  - The `cargo xtask run [--el2]` subcommand: build, then run QEMU with the terminal attached (quit with Ctrl-A X).
  - `scenarios::Scenario { name: &'static str, features: &'static [&'static str], machine: &'static str, expect: Expect, timeout: Duration }`. `cargo xtask test` builds and runs every scenario. It passes only if `check` returns `Ok` **and** the exit is `Exited(Some(0))`.

**Pinned values**
- **Link and load address:** `0x4008_0000`. QEMU loads a Linux Image at RAM base `0x4000_0000` + `text_offset`.
- **Image header:** the 64 bytes come from `Documentation/arch/arm64/booting.rst` §4:
  - `code0` = `b primary_entry`, `code1` = `0`.
  - `text_offset` = `0x80000`, `image_size` = `.quad __image_size`. This is an absolute symbol defined in `kernel.ld`, because the assembler can't subtract two undefined symbols. Linux does the same with `_kernel_size_le`. The image size must include `.bss` and the stack.
  - `flags` = `0x2` (little-endian, 4 KiB pages).
  - `res2`–`res4` = `0`, `magic` = `0x644d5241` ("ARM\x64"), `res5` = `0`.
- **Linker script `kernel.ld`:**
  - `ENTRY(_start)`, starting at `. = 0x40080000`.
  - Sections in order: `__image_start`, `.text` (with `KEEP(*(.text.head))` first), `.rodata`, `.data`.
  - Then `.bss (NOLOAD)`, 16-aligned, between `__bss_start` and `__bss_end`.
  - Then `.stack (NOLOAD)`: 64 KiB, 16-aligned, ending at `__stack_top`. Then `__image_end`, then `__image_size = __image_end - __image_start;`.
  - Discard `.comment`, `.eh_frame*` and `.note*`.
- **Entry assembly in `head.rs`** (`global_asm!`, `.section .text.head`), in this order:
  1. Save `x0` in `x19` first, before anything can clobber it.
  2. Set `sp = __stack_top`.
  3. Zero `[__bss_start, __bss_end)` 8 bytes at a time, without touching the stack.
  4. Set `x0 = x19`, `x1 = adr _start` (the runtime address), `x2 = ldr =_start` (the linked address).
  5. `bl __rust_boot`, then loop on `wfe`.
- **`__rust_boot`:** `extern "C" fn __rust_boot(dtb_addr: usize, runtime_start: usize, linked_start: usize) -> !`, in `foundation/src/arch/aarch64/boot.rs`.
  - If `runtime_start != linked_start`, write `"rust_kernel: loaded at wrong address (check text_offset)\n"` using `Console::write_str` only, with no formatting, then `power_off()`.
  - The reason: formatting tables hold absolute addresses, so formatted output breaks when the kernel runs somewhere other than where it was linked.
- **`current_el()`:** `mrs CurrentEL`, then `(value >> 2) & 3`.
- **PL011 UART** (`include/linux/amba/serial.h`):
  - Base `0x0900_0000`. This is QEMU `virt`'s UART; M2 reads it from the DTB instead.
  - `UART01x_DR = 0x00`, `UART01x_FR = 0x18`, `UART01x_FR_TXFF = 1 << 5`.
  - Spin while TXFF is set, then write the byte. Use 32-bit volatile access.
- **PSCI power-off** (`include/uapi/linux/psci.h`):
  - `PSCI_0_2_FN_SYSTEM_OFF = 0x8400_0008` in `x0`.
  - Use `hvc #0` when `current_el() == 1` and `smc #0` when it is 2. That matches QEMU `virt`'s PSCI conduit with and without `virtualization=on`. M2 replaces this with the DTB `/psci` `method`, the way `psci.c` does it.
  - If the call returns, loop on `wfe`.
- **Panic handler** (`kernel/src/kernel/panic.rs`, `#[cfg(target_os = "none")]`), matching `kernel/panic.c`. It prints three lines, then calls `power_off()`:
  - `pr_emerg!("Kernel panic - not syncing: {}", info.message())`
  - `pr_emerg!("  at {}:{}", file, line)`, when the location is known
  - `pr_emerg!("---[ end Kernel panic - not syncing: {} ]---", info.message())`
- **`start_kernel` output in this task:**
  - `rust_kernel: booting on {foundation::arch::NAME}`
  - `rust_kernel: CurrentEL = EL{n}`
  - `rust_kernel: boot OK, powering off`
- **`vmlinux/build.rs`:** for `target_os == "none"` and `target_arch == "aarch64"` only, print `cargo:rustc-link-arg-bins=-T<abs path to foundation/src/arch/aarch64/kernel.ld>` and `cargo:rerun-if-changed=<same>`.

- [x] **Step 1: Study the boot path and write `docs/study/01-arm64-boot.md`** from the template. Read these at v7.3 and cite lines:
  - `Documentation/arch/arm64/booting.rst` §4–5: the header, plus the register and MMU state at entry (MMU off, x0 = DTB physical address).
  - `arch/arm64/kernel/head.S`: from `primary_entry` to `__primary_switched` to `start_kernel`. Note `init_kernel_el`, which we defer.
  - `arch/arm64/kernel/vmlinux.lds.S`
  - `init/main.c` `start_kernel`: list its first 15 calls and say which milestone each corresponds to.
  - `include/linux/amba/serial.h` and the TX path of `drivers/tty/serial/amba-pl011.c`.
  - `uart_console_write` in `drivers/tty/serial/serial_core.c`.
  - `panic()` in `kernel/panic.c`.
  - `psci_sys_poweroff` in `drivers/firmware/psci/psci.c`.
  - `fdt_check_header` in `scripts/dtc/libfdt/fdt.c`.
  - Under "Our Rust design", explain the `foundation`/`kernel` split, `entry!`, and why the target is softfloat.

- [x] **Step 2: Write the failing scenario.** In `xtask/src/scenarios.rs`, add `boot`:
  - `features: &[]`, `machine: "virt"`, `timeout: 10 s`
  - `must_contain: ["rust_kernel: booting on aarch64", "rust_kernel: CurrentEL = EL1", "rust_kernel: boot OK, powering off"]`
  - `must_not_contain: ["Kernel panic", "wrong address"]`

  Then wire `cargo xtask test` to run the scenarios after the host tests.

- [x] **Step 3: Run it and confirm it fails**

Run: `cargo xtask test`. Expected: FAIL. The `vmlinux` package doesn't exist yet, so the build fails.

- [x] **Step 4: Implement `foundation`, `kernel` and `vmlinux`.**
  - Use the pinned values above.
  - `vmlinux/src/main.rs` is exactly `#![no_std]`, `#![no_main]`, `foundation::entry!(kernel::init::main::start_kernel);`.
  - `kernel` depends on `foundation`. `vmlinux` depends on both.
  - No atomics, and no `static mut` beyond what `foundation` needs.

- [x] **Step 5: Check the Image header**

Run: `cargo xtask build && xxd -l 64 target/aarch64-unknown-none-softfloat/debug/Image`

Expected: bytes 0x08–0x0f are `0000 0800 0000 0000` (text_offset `0x80000`, little-endian), and bytes 0x38–0x3b are `4152 4d64` ("ARMd").

- [x] **Step 6: Run it and confirm it passes**

Run: `cargo xtask test`. Expected: the host tests pass and `scenario boot: ok`.

- [x] **Step 7: Try it by hand**

Run: `cargo xtask run`. Expected: the three lines print, and QEMU exits on its own.

- [x] **Step 8: Commit**

```bash
git add Cargo.toml foundation kernel vmlinux xtask README.md docs/study/01-arm64-boot.md
git commit -m "feat(arm64): boot Linux-format Image in QEMU, PL011 console, PSCI power-off"
```

### Task 5: Hand the device tree from `foundation` to `kernel`

**Files:**
- Modify: `foundation/src/arch/aarch64/boot.rs`, `kernel/src/init/main.rs`, `kernel/Cargo.toml` (depend on `klib`), `xtask/src/scenarios.rs`

**Interfaces:**
- Consumes: `klib::fdt::{FdtHeader, FdtError, FDT_MAGIC, MAX_FDT_SIZE}` (Task 3) and `BootInfo` (Task 4).
- Produces: `BootInfo.dtb` is filled. `start_kernel` prints the DTB line.

**Rules**
- **`foundation`** is the raw-memory part.
  - It builds `dtb` from `dtb_addr` only if the address is non-zero and 8-aligned, the big-endian `u32` at the address equals `FDT_MAGIC`, and the big-endian `u32` at +4 is between 40 and `MAX_FDT_SIZE`.
  - When all of that holds, `dtb` is a slice of that length. Otherwise it is `&[]`.
  - `image` = `__image_start..__image_end`.
- **`kernel`** applies the policy, in safe code:
  1. If `dtb_addr..dtb_addr+dtb.len()` overlaps `image` (or `dtb_addr` is inside `image` when `dtb` is empty), panic with `device tree at {dtb_addr:#x} overlaps kernel image {start:#x}..{end:#x}`.
  2. Run `FdtHeader::parse(info.dtb)`. On `Err(e)`, panic with `no valid device tree at {dtb_addr:#x}: {e:?}`.
  3. Print `rust_kernel: dtb at {dtb_addr:#x}, totalsize {} bytes, version {}`.

- [x] **Step 1: Extend the `boot` scenario so it fails.** The `must_contain` list becomes `["rust_kernel: booting on aarch64", "rust_kernel: CurrentEL = EL1", "rust_kernel: dtb at 0x", "version 17", "rust_kernel: boot OK, powering off"]`.

- [x] **Step 2: Run it and confirm it fails**

Run: `cargo xtask test`. Expected: `scenario boot` FAIL, `missing (in order): "rust_kernel: dtb at 0x"`.

- [x] **Step 3: Implement the rules above.**

- [x] **Step 4: Run it and confirm it passes**

Run: `cargo xtask test`. Expected: all pass. Write the printed DTB address and size into `docs/study/01-arm64-boot.md`.

- [x] **Step 5: Commit**

```bash
git add foundation kernel xtask docs/study/01-arm64-boot.md
git commit -m "feat(boot): validate the device tree passed in x0 via klib"
```

### Task 6: Failure paths and the safety and dependency policy

**Files:**
- Create: `xtask/src/policy.rs`
- Modify: `kernel/Cargo.toml` (feature `test-panic = []`), `vmlinux/Cargo.toml` (feature `test-panic = ["kernel/test-panic"]`), `kernel/src/init/main.rs`, `xtask/src/scenarios.rs`, `xtask/src/main.rs`, `README.md`

**Interfaces:**
- Consumes: `Scenario`, `check`, `run_with_timeout`.
- Produces: `policy::external_crates(cargo_tree_output: &str, allowed: &[&str]) -> Vec<String>`.

**Kernel changes**
- In `start_kernel`, right after the CurrentEL line: if `current_el != 1`, `panic!("rust_kernel requires EL1, booted at EL{}", current_el)`.
- After the DTB line: `#[cfg(feature = "test-panic")] panic!("test-panic feature enabled");`

- [x] **Step 1: Write the failing tests and scenarios.**

In `policy.rs`:

```rust
#[test] fn only_workspace_crates_is_clean() {
    let tree = "vmlinux v0.1.0 (/r/vmlinux)\nfoundation v0.1.0 (/r/foundation)\nkernel v0.1.0 (/r/kernel)\nklib v0.1.0 (/r/klib)\n";
    assert!(external_crates(tree, &["vmlinux", "foundation", "kernel", "klib"]).is_empty());
}
#[test] fn flags_crates_io_dependency() {
    let tree = "vmlinux v0.1.0 (/r/vmlinux)\nspin v0.9.8\n";
    assert_eq!(external_crates(tree, &["vmlinux"]), vec!["spin".to_string()]);
}
```

Add these scenarios. Both exit as `Exited(Some(0))` because the kernel powers off.

| Scenario | Features | Machine | must_contain (in order) | must_not_contain |
|---|---|---|---|---|
| `panic` | `["test-panic"]` | `"virt"` | `"rust_kernel: booting on aarch64"`, `"Kernel panic - not syncing: test-panic feature enabled"`, `"---[ end Kernel panic - not syncing: test-panic feature enabled ]---"` | `"boot OK"` |
| `el2` | `[]` | `"virt,virtualization=on"` | `"rust_kernel: CurrentEL = EL2"`, `"Kernel panic - not syncing: rust_kernel requires EL1, booted at EL2"` | `"boot OK"` |

- [x] **Step 2: Run them and confirm they fail**

Run: `cargo xtask test`. Expected:
- `policy` fails to compile until `external_crates` exists.
- `scenario panic` fails, because the feature doesn't exist yet.
- `scenario el2` fails on `missing (in order): "Kernel panic - not syncing: rust_kernel requires EL1…"`.

- [x] **Step 3: Implement.**
  - Add the kernel changes above and `external_crates` (the first word of each non-empty line, keeping names not in `allowed`).
  - Then make `cargo xtask test` also run:
    - `cargo tree -p vmlinux --target aarch64-unknown-none-softfloat -e normal --prefix none`. Fail if `external_crates` returns anything outside `["vmlinux", "foundation", "kernel", "klib"]`.
    - `cargo clippy -p vmlinux -p foundation -p kernel -p klib --target aarch64-unknown-none-softfloat -- -D warnings`.

- [x] **Step 4: Run them and confirm they pass**

Run: `cargo xtask test`. Expected: host tests, policy, clippy and the scenarios `boot`, `panic` and `el2` all OK.

If `el2` prints its panic line and then reports `TimedOut`, the `smc` power-off conduit at EL2 is wrong, not the kernel. Check the `/psci` `method` in QEMU's DTB with `-machine virt,virtualization=on,dumpdtb=virt.dtb` and `dtc -I dtb virt.dtb`. Then change the conduit choice to match.

- [x] **Step 5: Prove the safety wall fires.** This is a one-time manual check.
  - Temporarily add `fn _probe() { unsafe {} }` to `kernel/src/lib.rs`.
  - Run `cargo xtask build`. Expected: an error saying an `unsafe` block is used.
  - Remove the probe. Write the error text into `docs/study/01-arm64-boot.md` under "Our Rust design".

- [x] **Step 6: Commit**

```bash
git add kernel vmlinux xtask README.md docs/study/01-arm64-boot.md
git commit -m "test: panic and EL2 failure paths; enforce no-crates and unsafe policy"
```

---

## After M1

1. Write `docs/study/02-memory.md` from the M2 row of the roadmap.
2. Write `docs/superpowers/plans/<date>-m2-memory.md`, which includes the first real use of spinlocks and the MMU.
3. M2's plan must remove the three temporary M1 shortcuts:
   - the hard-coded UART base,
   - the PSCI conduit chosen by exception level,
   - no atomics before the MMU is on.
