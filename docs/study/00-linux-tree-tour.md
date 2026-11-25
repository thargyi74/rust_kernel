# 00 — A tour of the Linux tree

**Linux reference:** v7.3 (`c03c1a14507d`), in `reference/linux`

Run all commands from `reference/linux`. Each number below comes from the command shown next to it, so you can run it again yourself.

## Files read

- `MAINTAINERS:4-63`: what each field of an entry means. Entry `ARM64 PORT (AARCH64 ARCHITECTURE)` is at `MAINTAINERS:3945-3958`.
- `Documentation/process/2.Process.rst:20-100`: the release cycle and the merge window.
- `Documentation/process/2.Process.rst:225-260`: how patches travel up to Linus.

## How big is Linux?

The size of each top-level directory, counting only C, header, assembly and Rust lines:

```sh
git ls-files -z -- <dir> | grep -zE '\.(c|h|S|rs)$' | xargs -0 cat | wc -l
```

| Directory | Lines | What it is |
|---|---:|---|
| `drivers/` | 26,420,271 | Device drivers. This is **69%** of all code. |
| `arch/` | 2,525,837 | CPU-specific code, with 21 architectures |
| `sound/` | 1,682,975 | Audio drivers (ALSA) |
| `fs/` | 1,653,587 | The VFS core plus 76 subfolders, mostly filesystems |
| `include/` | 1,476,828 | Shared headers. `include/uapi/` is the user-space ABI. |
| `tools/` | 1,459,127 | User-space tools and selftests, not part of the kernel |
| `net/` | 1,304,812 | Networking stack |
| `kernel/` | 573,985 | Core: scheduler, fork/exit, signals, time, locking |
| `lib/` | 327,873 | Helper library: strings, lists, trees, compression |
| `mm/` | 228,917 | Memory management |
| `rust/` | 160,644 | Rust support (`kernel` crate, bindings) |
| `init/` | 4,771 | `start_kernel()` and early boot |
| **Total** | **38,268,390** | in 96,080 tracked files (`git ls-files \| wc -l`) |

**The parts we will read line by line** (same counting command):

| Part | Lines | Milestone |
|---|---:|---|
| `arch/arm64` | 182,562 | M1–M4, M7 |
| `mm` | 228,917 | M2, M4 |
| `kernel/sched` | 77,022 | M4 |
| `fs/*.c fs/*.h` (VFS core only) | 88,597 | M5 |
| `init` | 4,771 | M1 |
| `arch/x86` / `arch/riscv` | 471,510 / 68,931 | M8 / M9 |

We won't read all of these lines either. Inside each part we follow the code path our kernel needs, such as boot, then fork, then a page fault. We skip most of `drivers/`, `sound/` and `net/`. They are drivers for real hardware we don't have, and their size comes from breadth, not depth. One UART, one interrupt controller and one timer teach the driver model.

## How old is the history?

| Fact | Command | Result |
|---|---|---|
| Commits reachable from v7.3 | `git rev-list --count HEAD` | 1,485,420 |
| First commit | `git log --reverse --format='%h %ad %s' --date=short \| head -1` | `1da177e4c3f4 2005-04-16 Linux-2.6.12-rc2` |

Git history starts at 2.6.12-rc2 in April 2005. That is when Linus switched to git, which he wrote for Linux. The 2.6.12-rc2 source tree was imported as one big first commit. Earlier history, back to 0.01 in 1991, is not in this repository.

## How a release is made

The dates of each release (`git log -1 --format=%ad --date=short <tag>`):

| Tag | Date | Gap |
|---|---|---|
| v7.2 | 2026-08-16 | the previous release |
| v7.3-rc1 | 2026-08-30 | **2 weeks**: the merge window |
| v7.3-rc2 … rc7 | 2026-09-06 … 2026-10-11 | one every week (Sunday) |
| v7.3 | 2026-10-18 | 9 weeks after v7.2 |

This matches `Documentation/process/2.Process.rst:32-56`:
- **Merge window (about 2 weeks):** Linus pulls new features that were already prepared and tested in subsystem trees.
- **`-rc1`:** after it, only fixes are accepted.
- **`-rc2` onward:** a weekly release candidate until the tree is stable.

The numbers for v7.2..v7.3:

| Fact | Command | Result |
|---|---|---|
| Commits in the release | `git rev-list --count v7.2..v7.3` | 20,260 |
| …landed in the merge window | `git rev-list --count v7.2..v7.3-rc1` | 16,267 (80%) |
| Merge commits | `git rev-list --count --merges v7.2..v7.3` | 1,495 |
| Pulls made by Linus himself | `git log --first-parent --merges v7.2..v7.3 --format=%s \| wc -l` | 511 |
| Distinct author emails | `git log v7.2..v7.3 --format=%aE \| sort -u \| wc -l` | 3,120 |

**Code moves up a tree of maintainers.** `git log --first-parent` walks only the commits Linus made on his own branch. Almost all of them are merges like these:

```
Merge tag 'rust-fixes-7.3' of git://git.kernel.org/pub/scm/linux/kernel/git/ojeda/linux
Merge tag 'timers-urgent-2026-08-30' of git://git.kernel.org/pub/scm/linux/kernel/git/tip/tip
Merge tag 'drm-next-2026-08-29' of https://gitlab.freedesktop.org/drm/kernel
```

The flow works like this:
1. A developer emails a patch to a mailing list.
2. A subsystem maintainer applies it to their git tree.
3. The maintainer sends Linus a signed tag to pull (`2.Process.rst:232`).

Linus almost never applies patches himself. He merges trees. **Engineering technique:** scaling by trust and hierarchy, with git's merges doing the integration.

## Who owns what: `MAINTAINERS`

There are about 3,300 entries. One of them, `MAINTAINERS:3945`:

```
ARM64 PORT (AARCH64 ARCHITECTURE)
M:	Catalin Marinas <catalin.marinas@arm.com>     ← send patches to
M:	Will Deacon <will@kernel.org>
R:	Mark Rutland <mark.rutland@arm.com>           ← reviewer, CC them
L:	linux-arm-kernel@lists.infradead.org          ← mailing list
S:	Maintained                                    ← status
T:	git git://git.kernel.org/pub/scm/linux/kernel/git/arm64/linux.git
F:	arch/arm64/                                   ← files covered
X:	arch/arm64/boot/dts/                          ← excluded
```

`scripts/get_maintainer.pl <patch>` reads these entries to work out who should receive a patch.

## The directories we will study

- **`arch/`**: one folder per CPU, all behind the same generic interfaces. For example, every arch provides `switch_to()` and page-table helpers through `arch/<x>/include/asm/`. This split is the main reason Linux runs on 21 architectures. Our `foundation/src/arch/<arch>/` copies it.
- **`init/`**: `start_kernel()` in `init/main.c` calls the init function of every subsystem in order. It's small (4.7k lines), but it's the table of contents for the whole boot.
- **`kernel/`**: the parts of the core that don't depend on hardware: processes (`fork.c`, `exit.c`), the scheduler (`sched/`), signals, time, locking and `printk`.
- **`mm/`**: physical page allocation (`page_alloc.c`), slab allocators (`slub.c`), virtual memory areas (`mmap.c`) and page faults (`memory.c`).
- **`fs/`**: the VFS (`namei.c`, `open.c`, `read_write.c`, `dcache.c`, `inode.c`) plus one subfolder per filesystem. Filesystems plug in through tables of function pointers (`struct file_operations` and friends). In our design these become Rust traits.
- **`drivers/`**: we read only the PL011 UART, the GIC interrupt controller, the ARM timer and PSCI.
- **`include/`**: `include/linux/` is internal to the kernel. `include/uapi/` is the user-space ABI that must never break. Our system-call layer copies the `uapi` constants.
- **`lib/`**: data structures and helpers. Our `klib` crate plays the same role, and it can be unit-tested on the Mac.

## Engineering techniques worth stealing

- **Generic code over arch hooks.** The core never asks "which CPU am I on?" It calls an interface that each arch implements.
- **Ops tables.** A `struct` of function pointers (`file_operations`, `inode_operations`) gives polymorphism in C. Rust traits do the same job, and the compiler checks them.
- **Stable user ABI, unstable internal API.** `include/uapi/` never breaks user space, so the kernel's internal code can change freely.
- **Hierarchy plus a time-boxed cycle.** The 2-week merge window, then weekly `-rc` releases and a release every 9–10 weeks, keeps 3,000+ authors in sync.

## Our Rust design and why it differs

- The same top-level split. Linux path `X` ↔ our `kernel/src/X`. CPU code goes to `foundation/src/arch/<arch>/`.
- Linux has no outside dependencies, and neither do our kernel crates.
- Linux's C code may use raw pointers anywhere. We put all `unsafe` code in `foundation`, so the rest of the kernel is checked by the compiler.

## Open questions

- How does `Kconfig` (Linux's build configuration) map onto Cargo features? This is deferred until we need options.
- The case-insensitive file system on macOS can't hold some netfilter header pairs (for example `xt_CONNMARK.h` and `xt_connmark.h`), so 13 files show as "modified". It's harmless for our study, because we don't read netfilter.
