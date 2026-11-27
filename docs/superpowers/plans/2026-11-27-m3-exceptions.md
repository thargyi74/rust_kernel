# M3 — Exceptions, Interrupts, Timer, First System Calls: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Real exception handling on ARM64:
- registers are saved and restored;
- kernel faults are reported with Linux's messages;
- a GICv2 and the virtual timer give a 250 Hz tick;
- a Rust user program runs at EL0, calls `write` and `exit` by Linux's numbers, and a user fault kills only that program.

**Architecture:**
- Pure data and decisions live in `klib` and are host-tested: ESR decoding and Linux's message tables, GIC and timer device-tree facts, tick math, the user window, user page bits and system-call decoding.
- `foundation` holds the assembly:
  - the vector table, `kernel_entry`/`kernel_exit`, and the jump into and out of EL0;
  - the GIC and timer registers;
  - the exception handlers that Rust code reaches from the assembly.
- `kernel` (still `#![forbid(unsafe_code)]`) holds the generic parts: the system calls, the tick, `jiffies` and the boot order.
- A new `user/` crate holds the user programs. They are embedded as flat binaries and copied into a fixed user window that is mapped at boot.

**Tech Stack:** Rust `stable` (tested with 1.100.0), `core` + `alloc`, `aarch64-unknown-none-softfloat` (also for the user programs), QEMU 11.1.3 `virt` with `cortex-a72` and 512 MiB, and `llvm-objcopy` from `llvm-tools`.

**Spec:** `docs/study/03-exceptions.md` (the study note, with the user's answers), together with the M3 row of the main plan, `docs/superpowers/plans/2026-11-25-rust-kernel.md`. Executors read both.

## Decisions

**Made by the user**
- 2026-11-27: option A for all five open questions of the study note:
  1. A new `user/` crate. `xtask` builds it, links it at `0x40_0000` and makes a flat binary. The kernel embeds it with `include_bytes!` and copies it into user pages.
  2. `unsafe` is allowed in one small file of the user crate (the `svc` stubs, like Linux's `nolibc`), and `cargo xtask test` checks that no other user file uses it.
  3. `run_user()` returns to `start_kernel` with "exited with code N" or "killed by signal". `foundation` saves a small kernel context before `eret` and resumes it at the end.
  4. A small `copy_from_user` now. It checks that the range lies inside the user window, not just "below `TASK_SIZE`", because our kernel is in the low half too.
  5. `HZ = 250`, periodic tick only. The test enables IRQs and waits with `wfi` until `jiffies` reaches 25.
- 2026-11-27, smaller choices (the user did not object):
  - interrupts run on the current kernel stack;
  - a guard page goes below the 64 KiB boot stack;
  - the wrong `entry.S:514-529` citation is fixed;
  - the Redox note's question 2 (a register check across preemption) is for M4.
- Everything in the main plan's and the M2 plan's Decisions still holds: ARM64 first, the safety wall, no crates, GPL-2.0-only, Native execution, commits on `main`, and the sealed `BootFdt` rule (M2 Decision 7).

**Proposed by Claude, not yet confirmed.** Each one is cheap to change before its task runs.
1. **The stack-overflow check comes with the guard page** (Task 3). This differs from the note's table, which said "not in M3".
   - **Why:** a guard page alone gives no report. On an overflow, the exception entry pushes `pt_regs` into the guard page and faults again. The nesting goes on until `sp` reaches `.bss`, which the handler then overwrites.
   - Linux's `tbnz sp, #THREAD_SHIFT` check (`entry.S:57-67`) needs only a stack aligned to twice its size, which the boot stack can be.
   - **To drop it:** skip Task 3. The guard page then moves to M4 as well.
2. **A fixed user window, mapped once by `paging_init`:**
   - 64 KiB of code at `0x40_0000` (user read-execute);
   - 16 KiB of stack ending at `0x80_0000` (user read-write);
   - both backed by pages from the boot memblock.
   
   Every program runs in the same window, one after the other. User programs may have no writable statics: the code window is read-execute, and the user linker script checks this. M4's address spaces replace the window.
3. **The GIC is probed from the `BootFdt` before `paging_init`** and mapped in the boot tables, as the console UART is. `boot_mappings` takes a list of device ranges instead of one UART. Linux `ioremap`s the GIC later; we have no `ioremap` yet.
4. **The user crate:**
   - one package, `user/`, with three binaries: `hello`, `fault` and `undef`;
   - each binary includes the shared `user/src/sys.rs` through `#[path]`;
   - each crate root has `#![deny(unsafe_code)]`, and only the `mod sys` line has `#[allow(unsafe_code)]`;
   - `sys.rs` is the only file with the word `unsafe`, and a host-tested `xtask` check enforces this.
5. **The handler interface:**
   - `kernel` registers one system-call handler with `foundation` at boot. It returns `SyscallAction::Return(i64)` or `SyscallAction::Exit(u8)`.
   - `exit` and user faults end `run_user` through a saved `CpuContext` (Linux `struct cpu_context`) held in a `foundation` static. There is one CPU until M7.
6. **A kernel fault panics** with `"<str>: Fatal exception"`. That is Linux's text when `panic_on_oops` is set: we have no task to kill.
7. **The tick stays on while user programs run.** `hello` spins long enough that timer interrupts arrive from EL0, and the kernel checks that at least one did. That covers the `el0t_64_irq` path.
8. **No new read-modify-write atomics.**
   - These counters have one writer and use a plain `load` + `store`: `jiffies`, the next deadline, the die counter, the fatal re-entry counter and the EL0-interrupt count.
   - `xtask::policy::ALLOWED_RMW` does not change.
9. **User read-write pages leave `PTE_RDONLY` clear.** Linux's `_PAGE_SHARED` sets `PTE_RDONLY | PTE_WRITE`, then relies on DBM or software dirty tracking to clear `RDONLY`. Copied as it is, that gives a read-only user stack on cortex-a72.
10. **Small differences from Linux's message text:**
    - no `" SMP"` at the end of `Internal error:`;
    - `pc : ` is printed in hex for the kernel too, because we have no symbols;
    - `show_regs` prints only `pc`, `lr` and `sp`.
11. **An SGI self-test** (SGI 15 sent to this CPU) checks the IRQ path in Task 6, before the timer exists.

## Execution in 3 parts

Each part runs Native (superpowers:executing-plans) in its **own fresh Claude Code session**, on the latest Opus model as in M2, unless the user picks another model (see "Cost estimate").

| Part | Tasks | Done when `cargo xtask test` is green with these scenarios |
|---|---|---|
| 1 | 1–4: ESR tables, exception entry and kernel fault reports, stack guard, IRQ flags | `boot`, `panic`, `el2`, `fault`, `nested-fault`, `unmapped`, `early-lock`, `recursive-panic`, `oom`, `stack-overflow` |
| 2 | 5–7: GIC and timer facts, the GIC and the IRQ path, the timer tick | the above, with `boot` showing the IRQ self-test, the timer banner and the tick self-test |
| 3 | 8–11: user window and syscall decoding, the user crate, EL0 entry with `write`/`exit`, user faults and README | the above, with `boot` running `hello`, `fault` and `undef`; this meets the M3 exit test |

**Rules for every part.** These override executing-plans where they differ.
- **Start from the record, not from memory.** A fresh session remembers nothing of earlier parts. Read the ledger `.superpowers/sdd/2026-11-27-m3-exceptions/progress.md` (create it in Part 1) and `git log`. Begin at the part's first task.
- **Stop after the part's last task.** Don't start the next part, even though executing-plans says to run every task.
- **Review each part.**
  - At the end of the part, review its commits (from the BASE of its first task to HEAD) with one fresh reviewer subagent, `model: "opus"`.
  - Then do the single fix pass, as executing-plans says.
  - Add `Part <N>: reviewed (<base7>..<head7>)` to the ledger.
- **Keep the workspace.** Don't delete `.superpowers/sdd/2026-11-27-m3-exceptions/` until Part 3's review is done.
- **Finish with a short report:** rulings made, deferred minors, and the exact text that starts the next part.

**Start text** (paste into a fresh session; change the part number and tasks):

```
Run Part 1 of docs/superpowers/plans/2026-11-27-m3-exceptions.md (Tasks 1–4), following its "Execution in 3 parts" section.
```

## Cost estimate

Measured from the session logs (cache reads / cache writes / output):

| Work | Tokens |
|---|---|
| M2 Part 1 | 10.6M / 0.21M / 0.11M |
| M2 Part 2 | 17.1M / 0.28M / 0.16M |
| M2 Part 3, with Close | 12.8M / 0.22M / 0.11M |
| M2 build in total | about 40M |
| M1, for scale | about 22M / 0.30M / 0.15M |

M3 is bigger than M2. It has more assembly, and EL0 bugs are hard to see. Rough guess:

| Part | Cache reads |
|---|---|
| Part 1 | 12–18M |
| Part 2 | 10–14M |
| Part 3 | 15–22M |
| **Total** | **about 37–54M**, with about 0.7–1.0M cache writes and 0.35–0.5M output |

That is about 1.7–2.5 times M1. A part that runs into a debugging loop can double its share. The cheapest choice: run Part 2 (the most mechanical part) on Sonnet, and keep Opus for Parts 1 and 3 and for every review.

## Global Constraints

Every Global Constraint of the main plan and of the M2 plan still applies. These are the changes and additions for M3:

- **The exception path runs with the MMU off, too.** The `fault` and `nested-fault` scenarios fault before `paging_init`. So every fatal EL1 path must be free of read-modify-write atomics, locks, heap use and `dc zva`: the fault reports, `die`, the unhandled-exception panic and the bad-stack report. The disassembly policy keeps checking this.
- **No new read-modify-write** (Decision 8). `ALLOWED_RMW` stays as it is. `SpinLock` may be used where the MMU is surely on (the `run_user` path).
- **The vector table:**
  - 2 KiB aligned, 16 slots of 128 bytes, in Linux's order (`entry.S:513-533`);
  - each slot ends with `.org <start> + 128`, so the assembler fails if a slot grows too big (`entry.S:100`);
  - `pt_regs` offsets come from `core::mem::offset_of!` passed as `const` operands to `global_asm!` (`asm-offsets.c:72-81`);
  - `size_of::<PtRegs>()` is 288, a multiple of 16, checked with a `const` assert.
- **GIC values** (`arm-gic.h:10-62`, `arm-gic-common.h:12`, `irq-gic.c:521`):
  - default priority `0xa0`, priority mask `0xf0`;
  - EOI mode 0: one write to `GICC_EOIR` drops the priority and deactivates (`irq-gic.c:218-226`, `:1290-1294`).
- **Timer and tick:**
  - the virtual timer (`CNTV_*`);
  - INTID = PPI number + 16, so QEMU's virtual timer is INTID 27;
  - `HZ = 250` (`kernel/Kconfig.hz:8`);
  - the next deadline = the last deadline + `CNTFRQ / HZ` (`tick-common.c:131-146`).
- **User ABI** (`tools/include/nolibc/arch-arm64.h:25-36`): the number goes in `x8`, the arguments in `x0`–`x5`, then `svc #0`; the result comes back in `x0`, with a negative errno on error.
  - System calls: `write = 64`, `exit = 93`, `exit_group = 94` (`scripts/syscall.tbl:83, 120, 121`).
  - Error numbers: `EBADF = 9`, `EFAULT = 14` (`errno-base.h:13, 18`), `ENOSYS = 38` (`errno.h:18`).
  - Signals: `SIGILL = 4`, `SIGTRAP = 5`, `SIGBUS = 7`, `SIGKILL = 9`, `SIGSEGV = 11` (`include/uapi/asm-generic/signal.h:14-22`).
- **The user window** (Decision 2):
  - `USER_TEXT_BASE = 0x40_0000`, `USER_TEXT_SIZE = 0x1_0000`;
  - `USER_STACK_TOP = 0x80_0000`, `USER_STACK_SIZE = 0x4000`;
  - the first PSTATE is `PSR_MODE_EL0t = 0` (`uapi/asm/ptrace.h:32`).
- **User page descriptors** (Decision 9; bits from `pgtable-hwdef.h:167, 171` and `pgtable-prot.h:61-64`):
  - user read-execute = `pa | 0x0020_0000_0000_0fc3`;
  - user read-write = `pa | 0x0060_0000_0000_0f43`;
  - both include nG, as Linux's do.
- **`unsafe` in `user/`:** only in `user/src/sys.rs`, and every block has a `// SAFETY:` comment.
- **The kernel is built only through `cargo xtask`.** The kernel embeds the user binaries, whose directory xtask passes in `RUST_KERNEL_USER_DIR`. So a plain `cargo build -p vmlinux` now fails with the `env!` message.
- **Log lines copy Linux wherever Linux has one:**
  - `Unable to handle kernel %s at virtual address %016lx` (`fault.c:349-350`);
  - the `mem_abort_decode` and `data_abort_decode` lines (`fault.c:65-118`);
  - `Internal error: %s: %016lx [#%d]` (`traps.c:182-183`);
  - `Unhandled %s exception on CPU%d, ESR 0x%016lx -- %s` (`entry-common.c:195-197`);
  - `Insufficient stack space to handle exception!` and its lines (`traps.c:903-929`);
  - `pc : %016llx`, `lr : %016llx`, `sp : %016llx` (`process.c:226-230`);
  - `%s[%d]: unhandled exception: ` + `%s, ESR 0x%016lx, ` + the fault name (`traps.c:252-254`);
  - `arch_timer: cp15 timer running at %lu.%02luMHz (%s).` (`arm_arch_timer.c:9`, `:876-879`).
- **Commits:** each task's commit step adds only the paths it lists. Never commit `.openidea/` or anything under `reference/`.

## Review Focus

1. **A bad `write` buffer must give -EFAULT, never a kernel fault.** Three cases: a kernel address (`0x4008_0000`), a range that wraps past `u64::MAX`, and a range that starts inside the window and ends outside it. → Task 8, the `user_range_ok_*` tests; Task 10, `hello` step 3.
2. **An unknown system call must give -ENOSYS, and the kernel goes on.** That includes `x8 = u64::MAX`. → Task 8, `decode_unknown_numbers`; Task 10, `hello` step 2.
3. **A bad file descriptor must give -EBADF.** → Task 10, `hello` step 4 (fd 3).
4. **A user fault kills only that program.** The next program still runs, and `boot OK` follows. → Task 11: the `boot` scenario runs `hello`, `fault` and `undef` in order, then `boot OK`.
5. **A kernel fault before the MMU is on still reports and powers off.** That includes a fault inside the report itself. → Task 2: the `fault` and `nested-fault` scenarios, and the bounded re-entry counter.

---

## File Structure (after M3)

New and changed files only.

```
klib/src/arch/aarch64/esr.rs           EC/FSC fields, Linux's class and fault tables, decode, MemAbortInfo ↔ esr.h, traps.c, fault.c
klib/src/uapi/{mod.rs, errno.rs, signal.rs, unistd.rs}   Linux's user-visible numbers       ↔ include/uapi/asm-generic/
klib/src/drivers/{mod.rs, irqchip/{mod.rs, gic.rs}, clocksource/{mod.rs, arm_arch_timer.rs}}  DT facts, register map ↔ irq-gic.c, arm_arch_timer.c
klib/src/time.rs                       HZ, tick period, deadline advance            ↔ kernel/time/tick-common.c
klib/src/syscall.rs                    number + registers → typed call              ↔ syscall_wrapper.h, sys.c
klib/src/mm/{map.rs, layout.rs}        user Access variants, ImageLayout.stack, devices, user window
klib/src/arch/aarch64/pgtable.rs       user descriptors
foundation/src/arch/aarch64/entry.rs   vectors, kernel_entry/exit, __bad_stack, __run_user, PtRegs ↔ entry.S
foundation/src/arch/aarch64/traps.rs   the Rust handlers, die, kernel fault, bad stack ↔ entry-common.c, traps.c, fault.c
foundation/src/arch/aarch64/irqflags.rs  DAIF save/restore, wfi                      ↔ asm/irqflags.h
foundation/src/arch/aarch64/arch_timer.rs  CNTV_* and CNTFRQ registers              ↔ asm/arch_timer.h
foundation/src/arch/aarch64/vectors.rs   deleted (the M2 early reporter)
foundation/src/drivers/irqchip/{mod.rs, gic.rs}   GICv2                              ↔ drivers/irqchip/irq-gic.c
foundation/src/irq.rs                  probe/init, request_irq, enable_irq, flags   ↔ kernel/irq/, arch/arm64/kernel/irq.c
foundation/src/time.rs                 the timer as the kernel sees it              ↔ arm_arch_timer.c
foundation/src/user.rs                 run_user, copy_from_user, syscall handler    ↔ processor.h start_thread, uaccess.h
foundation/src/sync/spinlock.rs        + lock_irqsave
foundation/src/console.rs              + write_bytes
kernel/src/kernel/{irq.rs, syscall.rs, exit.rs}   IRQ self-test, invoke_syscall, sys_exit ↔ syscall.c, kernel/exit.c
kernel/src/fs/{mod.rs, read_write.rs}  sys_write                                    ↔ fs/read_write.c
kernel/src/time/{mod.rs, tick_common.rs}   time_init, jiffies, tick, self-test     ↔ kernel/time/tick-common.c
kernel/src/usr/{mod.rs, initramfs_data.rs} embedded user programs                  ↔ usr/initramfs_data.S
user/{Cargo.toml, build.rs, user.ld}, user/src/sys.rs, user/src/bin/{hello.rs, fault.rs, undef.rs}  ↔ tools/include/nolibc
```

Every `.rs` file starts with the SPDX line and a `//!` comment naming its Linux counterpart.

---

## Part 1 — Exceptions

### Task 1: ESR, fault and signal tables in `klib`

**Files:**
- Create: `klib/src/arch/aarch64/esr.rs`, `klib/src/uapi/mod.rs`, `klib/src/uapi/errno.rs`, `klib/src/uapi/signal.rs`
- Modify: `klib/src/arch/aarch64/mod.rs`, `klib/src/lib.rs` (`pub mod uapi;`)

**Interfaces:**
- Produces, in `klib::arch::aarch64::esr`:
  - EC constants (`esr.h:13-71`): `EC_UNKNOWN = 0x00`, `EC_FP_ASIMD = 0x07`, `EC_SVC64 = 0x15`, `EC_SYS64 = 0x18`, `EC_IABT_LOW = 0x20`, `EC_IABT_CUR = 0x21`, `EC_PC_ALIGN = 0x22`, `EC_DABT_LOW = 0x24`, `EC_DABT_CUR = 0x25`, `EC_SP_ALIGN = 0x26`, `EC_SERROR = 0x2f`, `EC_BRK64 = 0x3c`, all `u8`.
  - `pub const fn ec(esr: u64) -> u8` (bits 31:26) and `pub const fn fsc(esr: u64) -> u8` (bits 5:0).
  - `pub fn class_str(esr: u64) -> &'static str`: the **whole** `esr_class_str` table (`traps.c:832-878`), with `"UNRECOGNIZED EC"` for every gap.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct FaultInfo { pub signal: u8, pub name: &'static str }`.
  - `pub fn fault_info(esr: u64) -> FaultInfo`: the **whole** 64-entry `fault_info` table (`fault.c:917-982`), indexed by FSC. It keeps the signal and the name and drops the handler. Add `const _: () = assert!(FAULT_INFO.len() == 64);`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Exception { Svc { imm: u16 }, DataAbort { far: u64, fsc: u8, write: bool }, InstrAbort { far: u64, fsc: u8 }, Breakpoint { imm: u16 }, Undefined, Other { ec: u8 } }`.
  - `pub fn decode(esr: u64, far: u64) -> Exception`:
    - DABT and IABT map to `DataAbort` and `InstrAbort` from either EL, because the caller knows which vector it came from;
    - `EC_UNKNOWN` → `Undefined`;
    - `write` is WnR (bit 6), and `imm` is ISS bits 15:0;
    - everything else → `Other { ec }`.
  - `pub struct MemAbortInfo(pub u64)` with `Display`: Linux's `mem_abort_decode` and `data_abort_decode` (`fault.c:65-118`), line for line, joined with `\n` and with no trailing newline. Field positions come from `esr.h:78-176`; ISS2 is bits 55:32 (`esr.h:82-83`).
  - `pub fn kernel_fault_msg(esr: u64, far: u64) -> &'static str`: the choice made by `__do_kernel_fault` (`fault.c:432-452`):
    - for a permission fault (FSC `0x0c..=0x0f`): `"write to read-only memory"` if WnR is set, else `"execute from non-executable memory"` for an instruction abort, else `"read from unreadable memory"`;
    - otherwise, `far < 4096` → `"NULL pointer dereference"`;
    - otherwise → `"paging request"`.
- Produces, in `klib::uapi`:
  - `errno`: `pub const EBADF: i64 = 9; pub const EFAULT: i64 = 14; pub const ENOSYS: i64 = 38;`
  - `signal`: `pub const SIGILL: u8 = 4; SIGTRAP = 5; SIGBUS = 7; SIGKILL = 9; SIGSEGV = 11;` and `pub fn signal_name(sig: u8) -> &'static str`, which gives `"SIGSEGV"` and so on, and `"SIG?"` for any other number.

- [ ] **Step 1: Write the failing tests** in `esr.rs`, `signal.rs`:

```rust
#[test] fn class_strings_match_linux() {
    assert_eq!(class_str(0x9600_0004), "DABT (current EL)");
    assert_eq!(class_str(0x5600_0000), "SVC (AArch64)");
    assert_eq!(class_str(0xf200_0123), "BRK (AArch64)");
    assert_eq!(class_str(0x0200_0000), "Unknown/Uncategorized");
    assert_eq!(class_str(0x3f << 26), "UNRECOGNIZED EC");
}
#[test] fn fault_info_matches_linux() {
    let f = |fsc| fault_info(0x9200_0000 | fsc);
    assert_eq!(f(0x04), FaultInfo { signal: SIGSEGV, name: "level 0 translation fault" });
    assert_eq!(f(0x0f), FaultInfo { signal: SIGSEGV, name: "level 3 permission fault" });
    assert_eq!(f(0x10), FaultInfo { signal: SIGBUS, name: "synchronous external abort" });
    assert_eq!(f(0x21), FaultInfo { signal: SIGBUS, name: "alignment fault" });
    assert_eq!(f(0x3f), FaultInfo { signal: SIGKILL, name: "unknown 63" });
}
#[test] fn decode_sorts_the_classes() {
    assert_eq!(decode(0x5600_0000, 0), Exception::Svc { imm: 0 });
    assert_eq!(decode(0x9200_000f, 0x4008_0000), Exception::DataAbort { far: 0x4008_0000, fsc: 0x0f, write: false });
    assert_eq!(decode(0x9600_0046, 0x8), Exception::DataAbort { far: 0x8, fsc: 0x06, write: true });
    assert_eq!(decode(0x8200_0007, 0x1234), Exception::InstrAbort { far: 0x1234, fsc: 0x07 });
    assert_eq!(decode(0xf200_0123, 0), Exception::Breakpoint { imm: 0x123 });
    assert_eq!(decode(0x0200_0000, 0), Exception::Undefined);
    assert_eq!(decode(0x1e00_0000, 0), Exception::Other { ec: EC_FP_ASIMD });
}
#[test] fn mem_abort_info_text_matches_linux() {
    assert_eq!(MemAbortInfo(0x9600_0004).to_string(), "Mem abort info:
  ESR = 0x0000000096000004
  EC = 0x25: DABT (current EL), IL = 32 bits
  SET = 0, FnV = 0
  EA = 0, S1PTW = 0
  FSC = 0x04: level 0 translation fault
Data abort info:
  ISV = 0, ISS = 0x00000004, ISS2 = 0x00000000
  CM = 0, WnR = 0, TnD = 0, TagAccess = 0
  GCS = 0, Overlay = 0, DirtyBit = 0");
}
```

Also:
- `instruction_abort_has_no_data_abort_lines`: `MemAbortInfo(0x8600_000f)` has 6 lines, and its last line is `"  FSC = 0x0f: level 3 permission fault"`.
- `isv_data_abort_prints_access_size`: `MemAbortInfo(0x9300_0047)` (ISV set, SAS = 0) contains `"  Access size = 1 byte(s)"` and no `"ISV = 0"`.
- `kernel_fault_messages_match_linux`:

| ESR | FAR | Message |
|---|---|---|
| `0x9600_0004` | `0xffff_ff00_0000_0000` | `"paging request"` |
| `0x9600_0006` | `0x8` | `"NULL pointer dereference"` |
| `0x9600_004f` | any | `"write to read-only memory"` |
| `0x8600_000f` | any | `"execute from non-executable memory"` |
| `0x9600_000f` | any | `"read from unreadable memory"` |

- `signal_names`: `signal_name(SIGSEGV) == "SIGSEGV"`, `signal_name(SIGILL) == "SIGILL"`, `signal_name(99) == "SIG?"`.

- [ ] **Step 2: Run them and confirm they fail.** Run `cargo test -p klib`. Expected: compile errors, because `esr` and `uapi` don't exist yet.
- [ ] **Step 3: Implement** the tables, `decode`, `MemAbortInfo` and `kernel_fault_msg`.
- [ ] **Step 4: Run them and confirm they pass.** Run `cargo test -p klib`. Expected: all pass, the M2 tests too.
- [ ] **Step 5: Commit**

```bash
git add klib
git commit -m "feat(klib): ESR decoding with Linux's class and fault tables; uapi errno and signal numbers"
```

### Task 2: Real exception entry and Linux's kernel fault reports

**Files:**
- Create: `foundation/src/arch/aarch64/entry.rs`, `foundation/src/arch/aarch64/traps.rs`
- Delete: `foundation/src/arch/aarch64/vectors.rs` (`git rm`)
- Modify: `foundation/src/arch/aarch64/{mod.rs, boot.rs}`, `xtask/src/scenarios.rs`

**Interfaces:**
- Consumes: `esr::{decode, class_str, MemAbortInfo, kernel_fault_msg, EC_SYS64}` from Task 1.
- Produces, crate-private in `foundation::arch::aarch64`:
  - `#[repr(C)] pub(crate) struct PtRegs { pub regs: [u64; 31], pub sp: u64, pub pc: u64, pub pstate: u64, pub orig_x0: u64, pub syscallno: u64 }` (`ptrace.h:154-170`, cut down). Add `const _: () = assert!(size_of::<PtRegs>() == 288);`.
  - `pub(crate) fn install()`, as before: it writes `VBAR_EL1`, and `__rust_boot` still calls it first.
  - The handlers that the assembly calls, each `#[unsafe(no_mangle)] extern "C"`:
    - `el1h_64_sync_handler(regs: &mut PtRegs)` and `el1h_64_irq_handler(regs: &mut PtRegs)`;
    - `el0t_64_sync_handler(regs: &mut PtRegs) -> u64` and `el0t_64_irq_handler(regs: &mut PtRegs)`;
    - `unhandled_handler(regs: &mut PtRegs, vector: u64) -> !`.
  - The assembly labels `ret_to_kernel` and `ret_to_user`.
  - `pub(crate) fn die(s: &str, regs: &PtRegs, esr: u64) -> !` and `pub(crate) fn show_regs(regs: &PtRegs)`. Tasks 3 and 11 reuse them.

**Rules:**
- **One slot** (`kernel_ventry`, `entry.S:39-101`, without the trampoline part): `.balign 128`; `sub sp, sp, #PT_REGS_SIZE`; `b <stub>`; `.org <start> + 128`. Task 3 adds the overflow check here.
- **`kernel_entry el`** (`entry.S:197-333`, only what we need):
  1. Store `x0`–`x29` in pairs.
  2. For EL0: zero `x0`–`x29` (`clear_gp_regs`, `:221`), then `mrs x21, sp_el0`. For EL1: `add x21, sp, #PT_REGS_SIZE`.
  3. `mrs x22, elr_el1` and `mrs x23, spsr_el1`. Store `x30` and `x21` as `regs[30]` and `sp`, and `x22` and `x23` as `pc` and `pstate`.
  4. For EL0: `syscallno = -1` (`NO_SYSCALL`, `ptrace.h:93`, `entry.S:308`).
- **`kernel_exit el`** (`entry.S:335-457`):
  1. Load `pc` and `pstate` into `elr_el1` and `spsr_el1`. For EL0, also load `sp` into `sp_el0`.
  2. Reload `x0`–`x30` and `add sp, sp, #PT_REGS_SIZE`.
  3. `eret`, then `dsb nsh` + `isb`. Those two are the fallback of Linux's `sb` (`assembler.h:100-108`): cortex-a72 has no FEAT_SB.
- **Stubs** (`entry_handler`, `entry.S:564-598`): `kernel_entry N`; `mov x0, sp`; `bl <handler>`; `b ret_to_kernel` or `b ret_to_user`. An unhandled stub also does `mov x1, #<slot index>` and never returns.
- **Which slot goes where** (`entry.S:513-533`):
  - slot 4 (el1h sync) → `el1h_64_sync_handler`;
  - slot 5 (el1h irq) → `el1h_64_irq_handler`;
  - slot 8 (el0t sync) → `el0t_64_sync_handler`;
  - slot 9 (el0t irq) → `el0t_64_irq_handler`;
  - the other 12 → `unhandled_handler`. Its description follows Linux's `UNHANDLED` macro (`entry-common.c:203-208`): `"64-bit el1t sync"`, `"64-bit el1h fiq"`, `"64-bit el0t error"`, `"32-bit el0t irq"`, and so on.
- **`el1h_64_sync_handler`** (`entry-common.c:462-509`), on `decode(ESR_EL1, FAR_EL1)`:
  - `DataAbort` / `InstrAbort` → the kernel-fault report:
    - `Unable to handle kernel {kernel_fault_msg} at virtual address {far:016x}`;
    - then `MemAbortInfo(esr)`;
    - then `die("Oops", …)` (`fault.c:345-360`).
  - `Breakpoint` → `die("Oops - BRK", …)` (`debug-monitors.c:261-267`).
  - `Undefined` or `Other { ec: EC_SYS64 }` → `die("Oops - Undefined instruction", …)` (`traps.c:476-488`).
  - Anything else → the unhandled panic with `"64-bit el1h sync"`.
- **`die`** prints these, then calls `panic!("{s}: Fatal exception")` (Decision 6):
  1. `Internal error: {s}: {esr:016x} [#{n}]` (`traps.c:182-183`). `n` counts dies, with a load and a store.
  2. `show_regs`.
- **`show_regs`** prints `pc : {pc:016x}`, `lr : {regs[30]:016x}` and `sp : {sp:016x}`.
- **The unhandled panic** (`entry-common.c:188-201`) prints these, then calls `panic!("Unhandled exception")`:
  1. `Unhandled {desc} exception on CPU0, ESR 0x{esr:016x} -- {class_str}`;
  2. `show_regs`.
- **`el1h_64_irq_handler`, `el0t_64_sync_handler` and `el0t_64_irq_handler`** take the unhandled-panic path with their Linux names until Tasks 6 and 10.
- **Bounded re-entry** (Review Focus 5; the M2 rule). Every fatal path first checks `static FATAL: AtomicU8`, with a plain load and store:
  - 0 → set 1 and report;
  - 1 → set 2 and `power::power_off()`;
  - anything higher → `halt()`.
- **Prints** use `writeln!(console::Console, …)`, which takes no lock and uses no heap.
- **The citation fix:** the new module comment cites `entry.S:513-533`.

- [ ] **Step 1: Make the scenarios fail.** In `xtask/src/scenarios.rs`:
  - `fault`:
    - `must_contain`: `["Internal error: Oops - BRK: 00000000f2000123 [#1]", "pc : 00000000400", "Kernel panic - not syncing: Oops - BRK: Fatal exception"]`;
    - `must_not_contain`: `["boot OK"]`.
  - `unmapped` `must_contain`: `["rust_kernel: MMU on", "Unable to handle kernel paging request at virtual address ffffff0000000000", "Mem abort info:", "  ESR = 0x0000000096", "  EC = 0x25: DABT (current EL), IL = 32 bits", "  FSC = 0x04: level 0 translation fault", "Internal error: Oops: 0000000096", "Kernel panic - not syncing: Oops: Fatal exception"]`.
    - Why level 0: `0xffff_ff00_0000_0000` uses entry `0x1fe` of the `TTBR1` level-0 table, and only entry 0 (the linear map) is filled.
  - `boot` `must_not_contain`: replace `"early exception"` with `"Internal error"`, `"Unhandled"` and `"Unable to handle kernel"`.
  - `nested-fault`: unchanged.
- [ ] **Step 2: Run and confirm they fail.** Run `cargo xtask test`. Expected: `scenario fault: missing (in order): "Internal error: Oops - BRK: 00000000f2000123 [#1]"`.
- [ ] **Step 3: Implement** `entry.rs`, `traps.rs` and the module wiring, and delete `vectors.rs`.
- [ ] **Step 4: Run and confirm they pass.** Run `cargo xtask test`. Expected:
  - `boot`, `panic`, `el2`, `fault`, `nested-fault`, `unmapped`, `early-lock`, `recursive-panic` and `oom` are all `ok`;
  - the disassembly policy passes.
- [ ] **Step 5: Check that the reporter is gone.** Run `grep -rn "early_exception\|early_vectors" foundation`. Expected: no output.
- [ ] **Step 6: Commit**

```bash
git add foundation xtask
git commit -m "feat(arm64): exception entry with pt_regs save/restore; Linux's kernel fault, BRK and unhandled reports"
```

### Task 3: Boot-stack guard page and Linux's overflow check (Decision 1)

**Files:**
- Modify: `foundation/src/arch/aarch64/{kernel.ld, entry.rs, traps.rs, paging.rs}`, `foundation/src/mm/mod.rs`, `klib/src/mm/layout.rs`, `kernel/src/mm/mm_init.rs`, `kernel/src/init/main.rs`, `kernel/Cargo.toml` and `vmlinux/Cargo.toml` (feature `test-stack-overflow`), `xtask/src/scenarios.rs`

**Interfaces:**
- Consumes: `PtRegs`, `show_regs` and `FATAL` from Task 2, and `class_str` from Task 1.
- Produces:
  - `klib::mm::layout::ImageLayout { text, rodata, data, stack }`. `data` now ends at the page-aligned end of `.bss`, and `stack` is the 64 KiB boot stack.
  - `#[unsafe(no_mangle)] extern "C" fn handle_bad_stack(regs: &mut PtRegs) -> !` in `traps.rs`.

**Rules:**
- **`kernel.ld`:**
  - after `.bss`: `. = ALIGN(4096); __data_end = .;`, then at least one page that nothing maps;
  - then `.stack (NOLOAD)` aligned to 128 KiB, holding `__stack_bottom`, 64 KiB, and `__stack_top`. This is Linux's `THREAD_ALIGN = 2 * THREAD_SIZE`;
  - add `ASSERT(__stack_bottom - __data_end >= 4096, …)` and `ASSERT(__stack_bottom % 0x20000 == 0, …)`.
  
  With that alignment, bit 16 of any valid `sp` is 0, and bit 16 of an `sp` in the 64 KiB below the stack is 1.
- **Mapping:** `boot_mappings` maps `stack` read-write in `Low`, as it maps `data`. The gap stays unmapped. Each image part, `stack` included, must lie inside one RAM range.
- **Memblock:** `BootMemblock::new` reserves `text.start()..stack.end()`.
- **The `Memory:` line:** rwdata becomes `data.size() + stack.size()`, so the number means the same as in M2.
- **The check in every vector slot**, copied from `entry.S:57-67` with `STACK_SHIFT = 16`:

  ```
  add sp, sp, x0; sub x0, sp, x0; tbnz x0, #16, 0f; sub x0, sp, x0; sub sp, sp, x0; b <stub>
  0: msr tpidr_el0, x0; sub x0, sp, x0; msr tpidrro_el0, x0; <sp = top of the overflow stack>; b __bad_stack
  ```

  - Linux's "already on the overflow stack" check is left out: `handle_bad_stack` never returns, and `FATAL` bounds any re-entry.
  - The slot must still pass its `.org` check.
- **The overflow stack** is 16 KiB, in `.bss`, 16-byte aligned. Linux uses 4 KiB (`memory.h:142`), but our debug-build panic path needs more.
- **`__bad_stack`** (`entry.S:535-561`):
  1. `mrs x0, tpidrro_el0`, then `sub sp, sp, #PT_REGS_SIZE`.
  2. `kernel_entry 1`.
  3. Put the real `sp` into the frame: `tpidr_el0 + PT_REGS_SIZE`.
  4. `mov x0, sp; bl handle_bad_stack`.
- **`handle_bad_stack`** (`panic_bad_stack`, `traps.c:903-929`) checks `FATAL` first. Then it prints these and calls `panic!("kernel stack overflow")`:
  1. `Insufficient stack space to handle exception!`
  2. `ESR: 0x{esr:016x} -- {class}`
  3. `FAR: 0x{far:016x}`
  4. `Task stack:     [0x{bottom:016x}..0x{top:016x}]`
  5. `Overflow stack: [0x{:016x}..0x{:016x}]` (there is no IRQ-stack line: we have no IRQ stack)
  6. `show_regs`
- **The test hook.** With the feature `test-stack-overflow`, the kernel calls `recurse(0)` right after `rust_kernel: MMU on`.
  - `recurse` is a safe function in `kernel/src/init/main.rs` with a `[u64; 64]` local kept alive by `core::hint::black_box`.
  - It has an exit that clippy cannot prove dead: `if buf[0] == u64::MAX { return 0 }`.
- **Before the MMU is on** the guard page is not enforced, so an overflow there stays silent, as before.

- [ ] **Step 1: Write the failing tests.**
  - In `layout.rs`, `qemu_image()` gains `stack: r(0x400c_0000, 0x400d_0000)`, with `data` ending at `0x400a_8000`.
  - New test `boot_mappings_leave_the_stack_guard_unmapped`: `Low` maps `stack` read-write, and no `Low` mapping touches `0x400a_8000..0x400c_0000`.
  - Fix the other layout tests for the new field.
  - New scenario `stack-overflow`:
    - features `["test-stack-overflow"]`;
    - `must_contain`: `["rust_kernel: MMU on", "Insufficient stack space to handle exception!", "Task stack:     [0x", "Kernel panic - not syncing: kernel stack overflow"]`;
    - `must_not_contain`: `["boot OK"]`.
- [ ] **Step 2: Run and confirm they fail.** Run `cargo test -p klib`, then `cargo xtask test`. Expected:
  - first, a compile error: `ImageLayout` has no `stack`;
  - then, after klib passes, `scenario stack-overflow` fails, because the feature doesn't exist yet.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.** Run `cargo xtask test`. Expected: every scenario is `ok`, `stack-overflow` included.
- [ ] **Step 5: Commit**

```bash
git add foundation klib kernel vmlinux xtask
git commit -m "feat(arm64): boot-stack guard page with Linux's THREAD_SHIFT overflow check and overflow stack"
```

### Task 4: IRQ flags and `lock_irqsave`

**Files:**
- Create: `foundation/src/arch/aarch64/irqflags.rs`, `foundation/src/irq.rs`
- Modify: `foundation/src/lib.rs` (`pub mod irq;`), `foundation/src/arch/{mod.rs, aarch64/mod.rs}`, `foundation/src/sync/{mod.rs, spinlock.rs}`, `kernel/src/lib/locking_selftest.rs`

**Interfaces:**
- Produces, in `foundation::irq`:
  - `#[must_use] pub struct IrqFlags(u64)`. It is neither `Clone` nor `Copy`, so a saved state is restored once.
  - `pub fn local_irq_enable()` (`msr daifclr, #3`) and `pub fn local_irq_disable()` (`msr daifset, #3`) (`irqflags.h:23-57`).
  - `pub fn local_irq_save() -> IrqFlags` reads DAIF and then masks. `pub fn local_irq_restore(flags: IrqFlags)` writes the saved DAIF back (`irqflags.h:163-197`).
  - `pub fn irqs_disabled() -> bool` reads DAIF.I (`PSR_I_BIT`, `0x80`, `uapi/asm/ptrace.h:46`).
  - `pub fn wait_for_interrupt()` runs `wfi`.
  - The `msr` and `wfi` asm leaves out `nomem`, so the compiler treats it as a memory barrier (Linux's `barrier()` / `"memory"` clobber).
- Produces, in `foundation::sync`: `SpinLock::lock_irqsave(&self) -> SpinLockIrqGuard<'_, T>`.
  - The guard derefs like `SpinLockGuard`.
  - On drop it unlocks first and then restores the flags (Linux `spin_unlock_irqrestore`).
  - It is re-exported from `foundation::sync`.

**Rules:**
- **The rule, in the doc comment:** a lock that any interrupt handler takes must be taken with `lock_irqsave` everywhere. In M3 no interrupt handler takes a lock, so `PAGES` and the heap keep `lock()`.
- **Where the new test runs.** The new self-test lines run inside `locking_selftest`, before the GIC is set up. No interrupt can arrive then even with DAIF.I clear, because the distributor is still disabled from reset.

- [ ] **Step 1: Write the failing test.** Add to `locking_selftest` (messages start with `"spinlock self-test: "`):

```rust
local_irq_enable();
{ let _g = lock.lock_irqsave(); assert!(irqs_disabled(), "...lock_irqsave left IRQs on"); }
assert!(!irqs_disabled(), "...unlock did not restore IRQs");
let outer = local_irq_save();
{ let _g = lock.lock_irqsave(); }
assert!(irqs_disabled(), "...a nested lock_irqsave turned IRQs on");
local_irq_restore(outer);
assert!(!irqs_disabled(), "...local_irq_restore lost the saved state");
local_irq_disable(); // the boot runs with IRQs off until time_init (Task 7)
```

- [ ] **Step 2: Run and confirm it fails.** Run `cargo xtask test`. Expected: a compile error, because `foundation::irq` and `lock_irqsave` don't exist yet.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm it passes.** Run `cargo xtask test`. Expected: every scenario is `ok`, and `boot` still shows `rust_kernel: spinlock self-test OK`.
- [ ] **Step 5: Commit**

```bash
git add foundation kernel
git commit -m "feat(sync): DAIF save/restore and SpinLock::lock_irqsave"
```

---

## Part 2 — Interrupts and the tick

### Task 5: GIC and timer facts in `klib`

**Files:**
- Create: `klib/src/drivers/mod.rs`, `klib/src/drivers/irqchip/{mod.rs, gic.rs}`, `klib/src/drivers/clocksource/{mod.rs, arm_arch_timer.rs}`, `klib/src/time.rs`
- Modify: `klib/src/lib.rs`

**Interfaces:**
- Consumes: `Fdt`, `Node` and `PhysRange` from M2.
- Produces, in `klib::drivers::irqchip::gic`:
  - Register offsets with Linux's names (`arm-gic.h:10-62`): `GIC_CPU_CTRL = 0x00`, `GIC_CPU_PRIMASK = 0x04`, `GIC_CPU_INTACK = 0x0c`, `GIC_CPU_EOI = 0x10`; `GIC_DIST_CTRL = 0x000`, `GIC_DIST_CTR = 0x004`, `GIC_DIST_ENABLE_SET = 0x100`, `GIC_DIST_ENABLE_CLEAR = 0x180`, `GIC_DIST_ACTIVE_CLEAR = 0x380`, `GIC_DIST_PRI = 0x400`, `GIC_DIST_TARGET = 0x800`, `GIC_DIST_CONFIG = 0xc00`, `GIC_DIST_SOFTINT = 0xf00`.
  - Values: `GICD_INT_DEF_PRI = 0xa0`, `GICC_INT_PRI_THRESHOLD = 0xf0`.
  - `pub const COMPATIBLE: [&str; 5] = ["arm,gic-400", "arm,cortex-a15-gic", "arm,cortex-a9-gic", "arm,cortex-a7-gic", "arm,pl390"]` (`irq-gic.c:1515-1523`).
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct GicV2 { pub dist: PhysRange, pub cpu: PhysRange }` and `pub enum GicError { NotFound, BadReg, TooSmall }`, with the same derives.
  - `pub fn find_gicv2(fdt: &Fdt<'_>) -> Result<GicV2, GicError>`:
    - it searches the tree depth-first for the first node with an `interrupt-controller` property and a compatible string from the list;
    - `reg` entries 0 and 1 are the distributor and the CPU interface (`irq-gic.c:1394, :1398`);
    - the distributor must be at least `0x1000` bytes and the CPU interface at least `0x2000` (`irq-gic.c:1305-1318`); otherwise `TooSmall`.
  - `pub fn intid(iar: u32) -> Option<u32>`: `iar & 0x3ff`, or `None` for 1020 and up (`arm-gic.h:38`, `irq-gic.c:344-345`).
  - `pub fn dt_hwirq(cells: [u32; 3]) -> Option<u32>` (`irq-gic.c:1081-1101`):
    - type 0 (SPI) → number + 32;
    - type 1 (PPI) → number + 16, only if the number is below 16;
    - anything else → `None`.
  - `pub fn nr_irqs(ctr: u32) -> u32`: `min(32 * ((ctr & 0x1f) + 1), 1020)` (`irq-gic.c:1205-1207`).
  - `pub fn sgi_to_self(sgi: u8) -> Option<u32>` gives `(0b10 << 24) | sgi` for `sgi < 16`. That is the `GICD_SGIR` value with TargetListFilter "this CPU only".
- Produces, in `klib::drivers::clocksource::arm_arch_timer`:
  - `ARCH_TIMER_CTRL_ENABLE = 1 << 0`, `ARCH_TIMER_CTRL_IT_MASK = 1 << 1`, `ARCH_TIMER_CTRL_IT_STAT = 1 << 2` (`arm_arch_timer.h:12-14`).
  - `pub const ARCH_TIMER_VIRT_PPI: usize = 2`, the index of the virtual timer in the binding's order (`arm_arch_timer.h:35-38`).
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum TimerError { NotFound, BadInterrupts, ZeroRate }`.
  - `pub fn virt_timer_intid(fdt: &Fdt<'_>) -> Result<u32, TimerError>`:
    - it finds the node compatible with `"arm,armv8-timer"`;
    - it reads `interrupts` as 3-cell entries, which is the GIC's `#interrupt-cells`;
    - it takes entry 2, which must be a PPI, through `dt_hwirq`;
    - otherwise `BadInterrupts`.
  - `pub struct Rate(pub u64)` with `Display` as `"{}.{:02}MHz"`, from `rate / 1_000_000` and `(rate / 10_000) % 100` (`arm_arch_timer.c:876-879`).
- Produces, in `klib::time`:
  - `pub const HZ: u64 = 250;`
  - `pub fn tick_period(rate: u64) -> Option<u64>` gives `rate / HZ`, or `None` if that is 0.
  - `pub fn advance(next: u64, period: u64, now: u64) -> (u64, u64)` is the loop of `tick_handle_periodic` (`tick-common.c:131-146`):
    - add `period` to `next` until `next > now`;
    - return that deadline and how many deadlines were skipped (extra ticks). A deadline equal to `now` counts as past: `clockevents_program_event` refuses it.

- [ ] **Step 1: Write the failing tests:**

```rust
const QEMU: &[u8] = include_bytes!("../../../testdata/qemu-virt-512m.dtb");
#[test] fn qemu_gic_is_gicv2_at_0x0800_0000() {
    let g = find_gicv2(&Fdt::new(QEMU).unwrap()).unwrap();
    assert_eq!((g.dist.start().as_u64(), g.dist.size()), (0x0800_0000, 0x1_0000));
    assert_eq!((g.cpu.start().as_u64(), g.cpu.size()), (0x0801_0000, 0x1_0000));
}
#[test] fn qemu_virt_timer_is_intid_27() { assert_eq!(virt_timer_intid(&Fdt::new(QEMU).unwrap()), Ok(27)); }
#[test] fn intid_masks_cpu_bits_and_spots_spurious() {
    assert_eq!(intid(0x1c1b), Some(27)); assert_eq!(intid(1019), Some(1019));
    assert_eq!(intid(1020), None); assert_eq!(intid(1023), None);
}
#[test] fn dt_hwirq_matches_linux() {
    assert_eq!(dt_hwirq([1, 11, 0x104]), Some(27)); assert_eq!(dt_hwirq([0, 1, 4]), Some(33));
    assert_eq!(dt_hwirq([1, 16, 4]), None); assert_eq!(dt_hwirq([2, 0, 0]), None);
}
#[test] fn advance_follows_the_grid() {
    assert_eq!(advance(1000, 100, 1050), (1100, 0));
    assert_eq!(advance(1000, 100, 1100), (1200, 1));
    assert_eq!(advance(1000, 100, 1350), (1400, 3));
}
```

Also:
- `nr_irqs_from_ctr`: `nr_irqs(0) == 32`, `nr_irqs(8) == 288`, `nr_irqs(31) == 1020`.
- `sgi_to_self_value`: `sgi_to_self(15) == Some(0x0200_000f)`, `sgi_to_self(16) == None`.
- `rate_prints_like_linux`: `62_500_000` → `"62.50MHz"`, `19_200_000` → `"19.20MHz"`, `1_000_000_000` → `"1000.00MHz"`.
- `tick_period_values`: `tick_period(62_500_000) == Some(250_000)`, `tick_period(100) == None`.
- With the M2 `Builder`:
  - `gic_with_one_reg_entry_is_bad_reg`;
  - `gic_cpu_interface_under_8k_is_too_small`;
  - `tree_without_gic_is_not_found`;
  - `timer_with_two_interrupt_entries_is_bad_interrupts`;
  - `timer_with_spi_virt_entry_is_bad_interrupts`.

- [ ] **Step 2: Run them and confirm they fail.** Run `cargo test -p klib`. Expected: compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and confirm they pass.** Run `cargo test -p klib`.
- [ ] **Step 5: Commit**

```bash
git add klib
git commit -m "feat(klib): GICv2 and arch-timer device-tree facts, register map, tick math"
```

### Task 6: GIC driver and the interrupt path

**Files:**
- Create: `foundation/src/drivers/irqchip/{mod.rs, gic.rs}`, `kernel/src/kernel/irq.rs`
- Modify: `foundation/src/drivers/mod.rs`, `foundation/src/irq.rs`, `foundation/src/arch/aarch64/{traps.rs, paging.rs}`, `foundation/src/mm/mod.rs`, `klib/src/mm/layout.rs`, `kernel/src/kernel/mod.rs`, `kernel/src/init/main.rs`, `xtask/src/scenarios.rs`

**Interfaces:**
- Consumes: Task 5's `gic` items and Task 4's flag functions.
- Produces:
  - `klib::mm::layout::boot_mappings(image, dtb, devices: &[PhysRange], ram, lm, emit)`. The `uart` argument becomes `devices`.
  - In `foundation::irq`:
    - `pub const NR_IRQS: u32 = 64;` (the SGIs, the PPIs and the first 32 SPIs).
    - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum IrqError { NoGic(GicError), NotProbed, BadIrq(u32), Busy(u32) }`.
    - `pub fn probe(fdt: &BootFdt) -> Result<(), IrqError>` finds the GIC and stores its ranges in atomics, as `console::init` does for the UART.
    - `pub(crate) fn mmio_ranges() -> Option<[PhysRange; 2]>`, which `paging_init` reads.
    - `pub fn init(mmu: &MmuOn) -> Result<(), IrqError>` (Linux `init_IRQ`).
    - `pub fn request_irq(intid: u32, handler: fn()) -> Result<(), IrqError>`.
    - `pub fn enable_irq(intid: u32) -> Result<(), IrqError>`.
    - `pub fn raise_sgi_self(sgi: u8) -> Result<(), IrqError>`.
    - `pub fn irqs_from_user() -> u64`.
    - `pub(crate) fn handle_irq(from_user: bool)`.
  - `kernel::kernel::irq::irq_selftest()`.

**Rules:**
- **`boot_mappings` with devices.** Each device is rounded out to whole pages and identity-mapped `ReadWrite` `Device` in `Low`. Every device is checked against RAM, every image part, the DTB and every other device; a clash is `Overlap`.
- **`paging_init`** passes the console UART plus, when `irq::probe` succeeded, both GIC ranges. Its signature doesn't change.
- **GIC init** (`irq-gic.c:440-525`, `irq-gic-common.c:91-142`). Each register is a 32-bit `read_volatile` / `write_volatile`.
  1. **Distributor:**
     - `CTRL = 0`, then `nr = nr_irqs(CTR)`;
     - for SPIs `32..nr`: target byte `0x01` (CPU 0), config 0 (level), priority `0xa0`, then clear active and clear enable;
     - `CTRL = 1`.
  2. **CPU interface:**
     - clear active and clear enable for INTIDs 0–31, and give them priority `0xa0`;
     - `PRIMASK = 0xf0`;
     - CPU `CTRL = 1`, with `EOImodeNS` 0.
- **The handler table** is `[AtomicUsize; NR_IRQS]` holding `fn()` addresses, with 0 for none.
  - `request_irq` gives `BadIrq` for `intid >= NR_IRQS` and `Busy` if a handler is already set.
  - It does a load, then a store: one CPU, before the IRQ is enabled, so no read-modify-write is needed.
  - Turning the stored address back into a `fn()` is this module's one `unsafe`.
- **`handle_irq`** (`gic_handle_irq`, `irq-gic.c:334-372`, with the EOI after the handler as in `handle_percpu_devid_irq`, `kernel/irq/chip.c:927-943`). In a loop:
  1. Read `INTACK`. If `intid(iar)` is `None`, stop.
  2. Call the handler. With no handler, print `rust_kernel: unexpected IRQ {intid}`.
  3. Write the **whole** `iar` to `EOI`.
  
  It runs with DAIF fully masked, as the exception was taken, and takes no lock. `from_user` adds 1 to the EL0 count, with a load and a store.
- **The vectors:** `el1h_64_irq_handler` calls `handle_irq(false)`, and `el0t_64_irq_handler` calls `handle_irq(true)`.
- **`enable_irq`** sets bit `intid % 32` in the `ENABLE_SET` word `intid / 32`. **`raise_sgi_self`** writes `sgi_to_self(sgi)` to `SOFTINT`, or returns `BadIrq` for 16 and up.
- **The boot order in `start_kernel`:**
  1. Right after `memblock_init`: `foundation::irq::probe(&fdt)`. On error, `panic!("no interrupt controller in the device tree: {:?}", e)`.
  2. After `heap_selftest`: `foundation::irq::init(&mmu)`, which panics on error. This follows Linux's order: `mm_core_init`, then `init_IRQ` (`init/main.c:1049`, `:1094`).
  3. Then `irq_selftest()`.
- **`irq_selftest`:**
  1. `request_irq(15, handler)`; this SGI is kept for the test, and M7 uses SGIs for IPIs. The handler stores `true` into an `AtomicBool`.
  2. `enable_irq(15)`, `local_irq_enable()`, `raise_sgi_self(15)`.
  3. Busy-wait with `spin_loop()` for at most 1,000,000 rounds until the flag is set. Not `wfi`: no other interrupt exists yet to wake it if the SGI came first.
  4. `local_irq_disable()`.
  5. Print `rust_kernel: irq self-test OK (SGI 15)`, or panic with `"irq self-test: SGI 15 never arrived"`.

- [ ] **Step 1: Write the failing tests.**
  - In `layout.rs`:
    - rename `uart_inside_ram_or_image_is_rejected` to `device_inside_ram_or_image_is_rejected`;
    - rename `dtb_and_uart_sharing_a_page_is_rejected` to `dtb_and_device_sharing_a_page_is_rejected`;
    - add `two_devices_sharing_a_page_are_rejected`;
    - add `boot_mappings_map_every_device`: the UART, GICD and GICC give three `Device` mappings in `Low`;
    - update `collect` and `qemu()`.
  - In `boot` `must_contain`, add `"rust_kernel: irq self-test OK (SGI 15)"` after `"rust_kernel: heap self-test OK"`.
- [ ] **Step 2: Run and confirm they fail.** Run `cargo test -p klib`, then `cargo xtask test`. Expected:
  - first, a compile error (`devices`);
  - then `scenario boot: missing (in order): "rust_kernel: irq self-test OK (SGI 15)"`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm they pass.** Run `cargo xtask test`. Expected: every scenario is `ok`.
- [ ] **Step 5: Commit**

```bash
git add foundation klib kernel xtask
git commit -m "feat(irq): GICv2 driver, IRQ vectors and request_irq; GIC mapped at boot; SGI self-test"
```

### Task 7: The architected timer and the tick

**Files:**
- Create: `foundation/src/time.rs`, `foundation/src/arch/aarch64/arch_timer.rs`, `kernel/src/time/{mod.rs, tick_common.rs}`
- Modify: `foundation/src/lib.rs`, `foundation/src/arch/{mod.rs, aarch64/mod.rs}`, `kernel/src/lib.rs`, `kernel/src/init/main.rs`, `xtask/src/scenarios.rs`

**Interfaces:**
- Consumes: from Task 5, `virt_timer_intid`, `TimerError`, `Rate`, `HZ`, `tick_period`, `advance` and the control bits; from Task 6, `request_irq` and `enable_irq`; from Task 4, `local_irq_enable` and `wait_for_interrupt`.
- Produces, in `foundation::time`:
  - `#[derive(Debug, Clone, Copy)] pub struct ArchTimer { pub rate: u64, pub intid: u32 }`.
  - `pub fn init(fdt: &BootFdt) -> Result<ArchTimer, TimerError>`:
    - it reads the INTID from the DT and the rate from `CNTFRQ_EL0` (`arch_timer.h:156`); a rate of 0 is `ZeroRate`;
    - it masks the timer (`CNTV_CTL_EL0 = IT_MASK`) so that an old deadline cannot fire.
  - `pub fn read_counter() -> u64`: `isb`, then `CNTVCT_EL0`.
  - `pub fn set_next_event(deadline: u64)`: `CNTV_CVAL_EL0 = deadline`, then `CNTV_CTL_EL0 = ENABLE` (IT_MASK clear), then `isb`.
  - `pub fn ack() -> bool` (`timer_handler`, `arm_arch_timer.c:560-574`): if `IT_STAT` is set, set `IT_MASK` (keeping `ENABLE`) and return `true`; otherwise return `false`.
- Produces, in `kernel::time::tick_common`:
  - `pub fn time_init(fdt: &BootFdt)` (Linux `time_init`, `init/main.c:1103`):
    1. `foundation::time::init`, then print `arch_timer: cp15 timer running at {Rate} (virt).`
    2. Compute the period with `tick_period`.
    3. `request_irq(intid, tick_handle_periodic)`.
    4. Set the next deadline to `read_counter() + period`, then `set_next_event` and `enable_irq`.
    
    Any error panics with `"arch_timer: {:?}"`.
  - `pub fn jiffies() -> u64` and `pub fn tick_selftest()`.
  - The private `fn tick_handle_periodic()`:
    1. If `ack()` is false, return.
    2. `do_timer(1)`.
    3. `let (next, extra) = advance(NEXT, PERIOD, read_counter())`.
    4. `do_timer(extra)`, then store `NEXT` and call `set_next_event(next)`.
    
    Reprogramming the deadline before `handle_irq` writes the EOI also lowers the level-triggered line (study note, step 7).
  - The private `fn do_timer(ticks: u64)` does `JIFFIES.store(JIFFIES.load(Relaxed) + ticks, Relaxed)`. There is one writer, the tick handler (Linux: `jiffies_64 += ticks`, `timekeeping.c:2828-2831`).

**Rules:**
- `JIFFIES`, `NEXT` and `PERIOD` are `AtomicU64`, with loads and stores only.
- **The boot order:**
  1. after `irq_selftest`: `time_init(&fdt)`;
  2. then `local_irq_enable()`. As in Linux (`init/main.c:1117-1118`), IRQs stay on for the rest of the boot;
  3. then `tick_selftest()`.
- **`tick_selftest`:**
  1. Record the counter and `jiffies`.
  2. Wait with `wait_for_interrupt()` until `jiffies` has grown by 25. The periodic tick always wakes `wfi`, so the check-then-sleep race costs at most one tick.
  3. Fewer than `24 * period` counter ticks elapsed → `panic!("tick self-test: 25 jiffies in only {} counter ticks (interrupt storm?)", elapsed)`. That is what happens if the timer is not masked or moved before the EOI.
  4. Print `rust_kernel: tick self-test OK (HZ=250, 25 jiffies in {ms} ms)`, with `ms = elapsed * 1000 / rate`.
- **The banner's frequency is not pinned.** QEMU's cortex-a72 should run at 62.5 MHz, but the scenario matches only the fixed text. Record the real line in the ledger.

- [ ] **Step 1: Make the scenario fail.** In `boot` `must_contain`, after `"rust_kernel: irq self-test OK (SGI 15)"`, add `"arch_timer: cp15 timer running at "`, `"MHz (virt)."` and `"rust_kernel: tick self-test OK (HZ=250, 25 jiffies in "`.
- [ ] **Step 2: Run and confirm it fails.** Run `cargo xtask test`. Expected: `scenario boot: missing (in order): "arch_timer: cp15 timer running at "`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm it passes.** Run `cargo xtask test`. Expected:
  - every scenario is `ok`;
  - the policy passes, with `jiffies` free of read-modify-write.
- [ ] **Step 5: Commit**

```bash
git add foundation kernel xtask
git commit -m "feat(time): virtual arch timer, 250 Hz periodic tick, jiffies and tick self-test"
```

---

## Part 3 — User mode

### Task 8: User window, user page bits and system-call decoding

**Files:**
- Create: `klib/src/uapi/unistd.rs`, `klib/src/syscall.rs`, `foundation/src/user.rs` (the window record only, for now)
- Modify: `klib/src/mm/{map.rs, layout.rs}`, `klib/src/arch/aarch64/pgtable.rs`, `klib/src/uapi/mod.rs`, `klib/src/lib.rs`, `foundation/src/lib.rs`, `foundation/src/mm/mod.rs`, `foundation/src/arch/aarch64/paging.rs`

**Interfaces:**
- Produces, in `klib::mm::map`:
  - `Access` gains `UserReadExecute` and `UserReadWrite`. There is no user read-write-execute variant, so W^X still holds.
  - `MapError` gains `UserDevice`, `UserOutsideRam` and `UserWindowClash { va: u64 }`.
- Produces, in `klib::arch::aarch64::pgtable`: `leaf_descriptor` for the user variants (Global Constraints):
  - `UserReadExecute` = `PTE_USER | PTE_RDONLY | PTE_NG | PTE_PXN`, with UXN clear;
  - `UserReadWrite` = `PTE_USER | PTE_NG | PTE_PXN | PTE_UXN`;
  - a user variant with `Device` → `UserDevice`;
  - add the constant `PTE_USER = 1 << 6`.
- Produces, in `klib::mm::layout`:
  - the four `USER_*` constants (Global Constraints);
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct UserWindow { pub text: PhysAddr, pub stack: PhysAddr }`: the physical pages behind the window;
  - `pub fn user_range_ok(addr: u64, len: u64) -> bool`:
    - `len == 0` → `true`;
    - otherwise `[addr, addr + len)`, computed without overflow, must lie wholly inside the text window or wholly inside the stack window.
    
    This is `access_ok` (`uaccess.h:29-50`), but checked against the window, because our kernel image is in the low half too (study note, design table);
  - `boot_mappings(image, dtb, devices, ram, user: &UserWindow, lm, emit)`, which has 7 parameters, within clippy's limit. It also emits:
    - `Low`, `USER_TEXT_BASE` → `user.text`, `USER_TEXT_SIZE`, `UserReadExecute`;
    - `Low`, `USER_STACK_TOP - USER_STACK_SIZE` → `user.stack`, `USER_STACK_SIZE`, `UserReadWrite`.
    
    Its checks:
    - both physical ranges page-aligned → otherwise `Misaligned`;
    - each inside one RAM range → otherwise `UserOutsideRam`;
    - neither virtual range overlaps any identity-mapped range → otherwise `UserWindowClash { va }`.
- Produces, in `klib::uapi::unistd`: `NR_WRITE = 64`, `NR_EXIT = 93`, `NR_EXIT_GROUP = 94` (`u64`).
- Produces, in `klib::syscall`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Syscall { Write { fd: u32, buf: u64, count: u64 }, Exit { code: i32 }, ExitGroup { code: i32 }, Unknown { nr: u64 } }`.
  - `pub fn decode(nr: u64, args: [u64; 6]) -> Syscall` is the typed unpacking that `SYSCALL_DEFINEx` does (`syscall_wrapper.h:49-70`): C `unsigned int` → `as u32`, `int` → `as i32`, `size_t` → unchanged.
- Produces, in `foundation::user`: `pub(crate) fn set_window(w: UserWindow)` and `pub(crate) fn window() -> Option<UserWindow>`, held in two `AtomicU64` with loads and stores.
- `paging_init` takes `USER_TEXT_SIZE` and `USER_STACK_SIZE` from the boot memblock (`alloc(size, PAGE_SIZE)`), passes them to `boot_mappings`, and calls `set_window` once the MMU is on.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn user_page_descriptors() {
    let pa = PhysAddr::new(0x4100_0000);
    let user = |access| Attrs { access, mem: MemType::Normal };
    assert_eq!(leaf_descriptor(pa, user(Access::UserReadExecute), 3), Ok(0x0020_0000_4100_0fc3));
    assert_eq!(leaf_descriptor(pa, user(Access::UserReadWrite), 3), Ok(0x0060_0000_4100_0f43));
    assert_eq!(leaf_descriptor(pa, Attrs { access: Access::UserReadWrite, mem: MemType::Device }, 3), Err(MapError::UserDevice));
}
#[test] fn user_range_ok_cases() {
    assert!(user_range_ok(0x40_0000, 0x10)); assert!(user_range_ok(0x40_0000, 0x1_0000));
    assert!(!user_range_ok(0x40_ffff, 2));            // runs off the text window
    assert!(!user_range_ok(0x4008_0000, 16));         // the kernel image
    assert!(!user_range_ok(u64::MAX - 1, 4));         // wraps
    assert!(!user_range_ok(0x40_0000, u64::MAX));
    assert!(user_range_ok(0x7f_fff0, 16)); assert!(!user_range_ok(0x7f_fff0, 17));
    assert!(user_range_ok(0, 0));
}
#[test] fn decode_unknown_numbers() {
    assert_eq!(decode(u64::MAX, [0; 6]), Syscall::Unknown { nr: u64::MAX });
    assert_eq!(decode(63, [0; 6]), Syscall::Unknown { nr: 63 });   // read: not in M3
}
#[test] fn decode_unpacks_like_c() {
    assert_eq!(decode(64, [1, 0x40_0100, 5, 0, 0, 0]), Syscall::Write { fd: 1, buf: 0x40_0100, count: 5 });
    assert_eq!(decode(64, [0x1_0000_0001, 0, 0, 0, 0, 0]), Syscall::Write { fd: 1, buf: 0, count: 0 });
    assert_eq!(decode(93, [0x12a, 0, 0, 0, 0, 0]), Syscall::Exit { code: 0x12a });
    assert_eq!(decode(93, [u64::MAX, 0, 0, 0, 0, 0]), Syscall::Exit { code: -1 });
    assert_eq!(decode(94, [3, 0, 0, 0, 0, 0]), Syscall::ExitGroup { code: 3 });
}
```

Also, in `layout.rs`:
- `boot_mappings_map_the_user_window`: two more `Low` mappings with the user attributes;
- `user_window_clashing_with_a_device_is_rejected`: a device at `0x40_0000` → `UserWindowClash { va: 0x40_0000 }`;
- `user_pages_outside_ram_are_rejected`;
- `misaligned_user_pages_are_rejected`;
- `collect` gains a `UserWindow` inside RAM (`text: 0x5000_0000`, `stack: 0x5001_0000`).

- [ ] **Step 2: Run them and confirm they fail.** Run `cargo test -p klib`. Expected: compile errors.
- [ ] **Step 3: Implement** klib, then the `foundation` side.
- [ ] **Step 4: Run them and confirm they pass.** Run `cargo test -p klib`, then `cargo xtask test`. Expected: all green. The `Memory:` line's reserved figure grows by 80 KiB; the scenario does not pin it.
- [ ] **Step 5: Commit**

```bash
git add klib foundation
git commit -m "feat(mm): user window mapped at boot with Linux's user page bits; system-call decoding"
```

### Task 9: The user crate and its build

**Files:**
- Create: `user/Cargo.toml`, `user/build.rs`, `user/user.ld`, `user/src/sys.rs`, `user/src/bin/{hello.rs, fault.rs, undef.rs}`, `kernel/src/usr/{mod.rs, initramfs_data.rs}`
- Modify: `Cargo.toml` (add `"user"` to `members`, not to `default-members`), `xtask/src/{main.rs, policy.rs}`, `kernel/src/lib.rs`

**Interfaces:**
- Produces, in `user/src/sys.rs`. This is the only file in `user/` with `unsafe`; its model is nolibc (`arch-arm64.h:25-36, 146-152`), and every block has a `// SAFETY:` comment.
  - `#[unsafe(no_mangle)] #[unsafe(link_section = ".text._start")] extern "C" fn _start() -> !` calls `exit(crate::main())`.
  - `pub fn syscall3(nr: u64, a0: u64, a1: u64, a2: u64) -> i64`: `svc #0` with `x8` = `nr`, `inout("x0")`, `in("x1")`, `in("x2")`, `options(nostack)`.
  - `pub fn write(fd: u32, buf: &[u8]) -> i64` and `pub fn write_raw(fd: u32, addr: u64, len: u64) -> i64`. The second takes a raw address, for the -EFAULT test.
  - `pub fn exit(code: i32) -> !`.
  - `pub fn read_u64(addr: u64) -> u64` does a `read_volatile`. The SAFETY comment says: the read faults, the kernel kills the program, and it never returns.
  - `pub fn undefined_instruction() -> !` runs `udf #0`.
  - `#[panic_handler]` writes `"user panic\n"` to fd 2 and exits with 101.
- Each bin starts with `#![no_std] #![no_main] #![deny(unsafe_code)]` and `#[allow(unsafe_code, dead_code)] #[path = "../sys.rs"] mod sys;`, and has `fn main() -> i32`.
  - **`hello`** runs these steps in order. If one fails, it prints `"hello: FAIL step N\n"` and returns N:
    1. `write(1, b"Hello from EL0!\n") == 16`.
    2. `syscall3(u64::MAX, 0, 0, 0) == -38`, then it prints `"hello: unknown system call gives -ENOSYS\n"`.
    3. `write_raw(1, 0x4008_0000, 16) == -14`, then it prints `"hello: write from a kernel address gives -EFAULT\n"`. `0x4008_0000` is the kernel's load address, from `kernel.ld`.
    4. `write(3, b"x") == -9`, then it prints `"hello: write to fd 3 gives -EBADF\n"`.
    5. `for i in 0..SPIN { core::hint::black_box(i); }` with `const SPIN: u64 = 5_000_000`, so timer interrupts arrive from EL0. Raise `SPIN` if Task 10's check fails, and record the value in the ledger.
    
    Then it returns 0.
  - **`fault`** prints `"fault: reading kernel memory at 0x40080000\n"`, calls `read_u64(0x4008_0000)`, and would return 3.
  - **`undef`** prints `"undef: running an undefined instruction\n"` and calls `undefined_instruction()`.
- **`user/user.ld`:**
  - `ENTRY(_start)`, `. = 0x400000;`;
  - `.text : { KEEP(*(.text._start)) *(.text .text.*) }`;
  - `.rodata : { *(.rodata .rodata.*) }`;
  - `.data : { __user_data_start = .; *(.data .data.*) *(.bss .bss.*) *(COMMON) __user_data_end = .; }`;
  - `ASSERT(_start == 0x400000, …)`, `ASSERT(__user_data_end == __user_data_start, "user programs may not have writable statics in M3")` and `ASSERT(. <= 0x410000, "larger than the 64 KiB user window")`;
  - `/DISCARD/` as in `kernel.ld`.
- **`user/build.rs`** works like `vmlinux/build.rs`: `cargo:rustc-link-arg-bins=-T<user.ld>`, only for `target_os = "none"`.
- Produces, in `xtask`:
  - `const USER_PROGRAMS: [&str; 3] = ["hello", "fault", "undef"];`
  - `fn build_user() -> Result<PathBuf, String>`:
    1. `cargo build -p user --bins --target aarch64-unknown-none-softfloat`;
    2. `llvm-objcopy -O binary` for each program, into `<target dir>/aarch64-unknown-none-softfloat/debug/user-bin/<name>.bin`;
    3. it returns that directory.
  - `build_image` calls `build_user()` first and sets `RUST_KERNEL_USER_DIR` on the `vmlinux` build.
  - `test()` calls `build_user()` before clippy and sets the same variable on the clippy command. Clippy gains `-p user`.
  - `pub fn policy::unsafe_outside(files: &[(String, String)], allowed: &str) -> Vec<String>`:
    - it gives the paths, other than `allowed`, whose text contains the identifier `unsafe` outside `//` comments;
    - an identifier is a maximal run of `[A-Za-z0-9_]`, so `unsafe_code` does not count.
  - `test()` runs it on every `.rs` file under `user/src` with `allowed = "user/src/sys.rs"`. It prints `xtask: policy: unsafe only in user/src/sys.rs`.
- Produces, in `kernel::usr::initramfs_data` (Linux `usr/initramfs_data.S:27`, `.incbin`): `pub static PROGRAMS: [(&str, &[u8]); 3]`, with `include_bytes!(concat!(env!("RUST_KERNEL_USER_DIR"), "/hello.bin"))` and the same for the other two.

- [ ] **Step 1: Write the failing tests** in `policy.rs`:
  - `unsafe_word_outside_the_allowed_file_is_found`;
  - `unsafe_code_lint_name_is_not_the_word_unsafe`;
  - `unsafe_in_a_line_comment_is_ignored`;
  - `the_allowed_file_may_use_unsafe`.
- [ ] **Step 2: Run them and confirm they fail.** Run `cargo test -p xtask`. Expected: a compile error, because `unsafe_outside` doesn't exist yet.
- [ ] **Step 3: Implement** the crate, the xtask steps and the embedding.
- [ ] **Step 4: Run and check.** Run `cargo xtask test`. Expected:
  - all green, with the new policy line;
  - `ls <target>/aarch64-unknown-none-softfloat/debug/user-bin/` shows three `.bin` files, each at most 64 KiB;
  - `llvm-objdump -d` of `hello` shows `_start` at `400000`.
- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock user xtask kernel
git commit -m "feat(user): user crate with nolibc-style svc stubs, built by xtask and embedded in the kernel"
```

### Task 10: Entering EL0, `write` and `exit`

**Files:**
- Create: `kernel/src/kernel/{syscall.rs, exit.rs}`, `kernel/src/fs/{mod.rs, read_write.rs}`
- Modify: `foundation/src/user.rs`, `foundation/src/console.rs`, `foundation/src/arch/aarch64/{entry.rs, traps.rs, mod.rs}`, `foundation/src/arch/mod.rs`, `kernel/src/lib.rs`, `kernel/src/kernel/mod.rs`, `kernel/src/init/main.rs`, `xtask/src/scenarios.rs`

**Interfaces:**
- Consumes:
  - from Task 8: `UserWindow`, `window()`, `user_range_ok` and `klib::syscall::decode`;
  - from Task 9: `PROGRAMS`;
  - from Task 2: `PtRegs` and `ret_to_user`;
  - from Task 6: `irqs_from_user`;
  - from Task 4: `local_irq_save` and `local_irq_restore`.
- Produces, in `foundation::user`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SyscallAction { Return(i64), Exit(u8) }`.
  - `pub type SyscallHandler = fn(nr: u64, args: [u64; 6]) -> SyscallAction;` and `pub fn set_syscall_handler(h: SyscallHandler)`. A second call panics.
  - `#[derive(Debug, Clone, Copy)] pub struct UserFault { pub signal: u8, pub pc: u64, pub lr: u64, pub sp: u64, esr: u64, far: u64, kind: FaultKind }`, with `Display`. The private `FaultKind` and the text are filled in by Task 11; here `Display` prints `"ESR 0x{esr:016x}"`.
  - `#[derive(Debug, Clone, Copy)] pub enum UserExit { Exited(u8), Killed(UserFault) }`.
  - `#[derive(Debug)] pub enum UserError { TooBig { len: usize }, NoWindow, NoSyscallHandler }`.
  - `pub fn run_user(image: &[u8]) -> Result<UserExit, UserError>`.
  - `pub fn access_ok(addr: u64, len: u64) -> bool`, built on `user_range_ok`.
  - `pub struct BadAddress;` and `pub fn copy_from_user(dst: &mut [u8], src: u64) -> Result<(), BadAddress>`.
- Produces, in `foundation::console`: `pub fn write_bytes(bytes: &[u8])`, with the same `\n` → `\r\n` rule as `write_str`.
- Produces, in `kernel`:
  - `kernel::syscall::invoke_syscall(nr: u64, args: [u64; 6]) -> SyscallAction`;
  - `kernel::exit::sys_exit(code: i32) -> SyscallAction`;
  - `fs::read_write::sys_write(fd: u32, buf: u64, count: u64) -> i64`;
  - in `init/main.rs`, `fn run_init_process(name: &str, image: &[u8], pid: u32)` (Linux `init/main.c:1472-1485`).

**Rules:**
- **`run_user`:**
  1. Check, in this order: a handler is set (`NoSyscallHandler`), the window exists (`NoWindow`), and `image.len() <= USER_TEXT_SIZE` (`TooBig`).
  2. Through the linear map, copy `image` into the text pages, then zero the rest of the text window and the whole stack.
  3. Make the new code visible to instruction fetch (Linux `caches_clean_inval_pou`):
     - `dc cvau` on every line of the text window, with the line size from `CTR_EL0.DminLine` (bits 19:16, `4 << n` bytes);
     - then `dsb ish`, `ic iallu`, `dsb ish`, `isb`.
     
     QEMU models no caches, so it can't catch a missing step. Review this code by reading it.
  4. Build the first frame, as `start_thread` does (`processor.h:339-345`):
     - every register 0;
     - `pc = USER_TEXT_BASE`, `sp = USER_STACK_TOP`, `pstate = 0` (EL0t, interrupts unmasked);
     - `syscallno = u64::MAX`.
     
     Also write 0 to `TPIDR_EL0` and `TPIDRRO_EL0`.
  5. `let flags = local_irq_save();`, then `__run_user(&frame)`, then `local_irq_restore(flags)`.
  6. Take the result from `static USER_EXIT: SpinLock<Option<UserExit>>`, which the EL0 handler fills. The MMU is surely on here.
- **`__run_user`** (`global_asm!` in `entry.rs`; the model is `cpu_switch_to`, `entry.S:817-844`):
  - It saves `x19`–`x28`, `x29`, `sp` and `lr` into `static KERNEL_CONTEXT: CpuContext`, where `#[repr(C)] struct CpuContext { x19..x28, fp, sp, pc }` (`processor.h:136-150`). It reaches the static with a `sym` operand. There is one CPU until M7.
  - Then `sub sp, sp, #PT_REGS_SIZE`, copy the 36 words of the frame there, and `b ret_to_user`.
  - After the `eret`, `SP_EL1` equals the saved `sp`. So every EL0 exception builds its frame just below it, where the kernel keeps nothing.
- **The EL0 sync stub** becomes:

  ```
  kernel_entry 0; mov x0, sp; bl el0t_64_sync_handler; cbnz x0, .Lend_user_run; b ret_to_user
  ```

  `.Lend_user_run` reloads the callee-saved registers, `sp` and `lr` from `KERNEL_CONTEXT` and does `ret`. `__run_user` thereby returns to `run_user`, with DAIF masked.
- **`el0t_64_sync_handler(regs) -> u64`** (`entry-common.c:752-761`, `:771-837`) returns 0 to go back to EL0, or 1 to end the run.
  - **`Svc`:**
    1. `regs.orig_x0 = regs.regs[0]` and `regs.syscallno = regs.regs[8]` (`syscall.c:67-68`).
    2. Unmask with DAIF = 0 (`DAIF_PROCCTX`, `daifflags.h:15`, `entry-common.c:757`).
    3. Call the handler with `x8` and `x0`–`x5`.
    4. Mask all of DAIF again.
    5. `Return(v)` → `regs.regs[0] = v as u64`, return 0. `Exit(c)` → store `Exited(c)`, return 1.
  - **Anything else:** store `Killed(UserFault { signal: SIGSEGV for aborts, else SIGILL, … })` and return 1. Task 11 makes the details right.
- **`invoke_syscall`** (`syscall.c:38-55` and `sys.c:39-63`; a `match` has no empty slots and needs no bounds check), on `decode(nr, args)`:
  - `Write` → `Return(sys_write(..))`;
  - `Exit` and `ExitGroup` → `sys_exit(code)`;
  - `Unknown { nr }` → `pr_info!("rust_kernel: unknown system call {}", nr)` and `Return(-ENOSYS)`.
- **`sys_exit`** returns `Exit((code & 0xff) as u8)` (`kernel/exit.c:1094-1097`).
- **`sys_write`** (`fs/read_write.c:667-689`, `:747-751`):
  - fds 0, 1 and 2 are the console, because Linux's init has `/dev/console` open read-write on all three (`init/main.c:1634`); any other fd → `-EBADF`;
  - `!access_ok(buf, count)` → `-EFAULT`, before any output;
  - otherwise copy in chunks of 256 bytes with `copy_from_user` into a stack buffer and `console::write_bytes`;
  - return `count`.
- **`copy_from_user`** fails with `BadAddress` unless `access_ok`. It reads through the window's linear-map alias, because M3 knows the window's physical pages. M4's `UserPtr` reads through the user mapping, with fault fix-ups.
- **`start_kernel`**, after `tick_selftest`:
  1. `set_syscall_handler(invoke_syscall)`;
  2. `run_init_process("hello", hello_bytes, 1)`, which prints `rust_kernel: hello[1] exited with code {c}`, or a killed line, or panics on a `UserError`;
  3. then `let n = irqs_from_user();`. If `n == 0`, `panic!("no timer interrupt arrived while hello ran at EL0")`. Otherwise print `rust_kernel: timer interrupts taken from EL0: {n}`.

- [ ] **Step 1: Make the scenario fail.** In `boot`:
  - `must_contain` gains, after the tick self-test line: `"Hello from EL0!"`, `"rust_kernel: unknown system call 18446744073709551615"`, `"hello: unknown system call gives -ENOSYS"`, `"hello: write from a kernel address gives -EFAULT"`, `"hello: write to fd 3 gives -EBADF"`, `"rust_kernel: hello[1] exited with code 0"` and `"rust_kernel: timer interrupts taken from EL0: "`;
  - `must_not_contain` gains `"hello: FAIL"` and `"user panic"`;
  - `timeout` becomes 20 s.
- [ ] **Step 2: Run and confirm it fails.** Run `cargo xtask test`. Expected: `scenario boot: missing (in order): "Hello from EL0!"`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm it passes.** Run `cargo xtask test`. Expected: every scenario is `ok`. Record in the ledger the EL0-interrupt count and the time `boot` takes.
- [ ] **Step 5: Commit**

```bash
git add foundation kernel xtask
git commit -m "feat(user): run_user enters EL0 and returns on exit; write/exit by Linux numbers; copy_from_user"
```

### Task 11: User faults are reported, and the kernel goes on; README

**Files:**
- Modify: `foundation/src/user.rs`, `foundation/src/arch/aarch64/traps.rs`, `kernel/src/init/main.rs`, `xtask/src/scenarios.rs`, `README.md`

**Interfaces:**
- Consumes: Task 1's `fault_info`, `class_str` and `signal_name`, and Task 10's `UserFault` and `run_init_process`.
- Produces: the final `UserFault` text, and the three-program boot.

**Rules:**
- **The EL0 cases that are not `Svc`** (`entry-common.c:771-837`). In each case `UserFault` keeps the signal, `pc`, `lr` (`x30`), `sp` and the text:
  - `DataAbort` / `InstrAbort` (`do_mem_abort`, `fault.c:984-1001`): the signal and name come from `fault_info(esr)`. The text is `"{class_str}, ESR 0x{esr:016x}, {name}"` (`traps.c:252-254`).
  - `Undefined`: `SIGILL`, and the text `"undefined instruction"` with no ESR part. Linux's `force_signal_inject` passes `err = 0` (`traps.c:409-436`, `:455-474`).
  - Anything else: `SIGILL`, and the text `"{class_str}, ESR 0x{esr:016x}, Bad EL0 synchronous exception"` (`bad_el0_sync`, `traps.c:889-898`).
- **`run_init_process` prints a kill like this.** Linux prints it only with `debug.exception-trace=1` (`traps.c:150`); the study note chose to always print it.

  ```
  {name}[{pid}]: unhandled exception: {fault}
  pc : {pc:016x}
  lr : {lr:016x}
  sp : {sp:016x}
  rust_kernel: {name}[{pid}] killed by signal {n} ({signal_name})
  ```

- **`start_kernel`** runs `hello` (pid 1), `fault` (pid 2) and `undef` (pid 3) in order, then prints `boot OK`.
- **Why `fault` gets a level 3 permission fault:**
  - `0x4008_0000` is not 2 MiB aligned, so the kernel text is mapped with 4 KiB pages;
  - those pages have AP[1] = 0;
  - so an EL0 load from there is a permission fault at level 3.
- **`README.md`:**
  - Status: M3 built;
  - the new expected boot output, from a real run;
  - the scenario list: ten now, with `stack-overflow` added and the new texts for `fault` and `unmapped`;
  - the crate map: add `user`;
  - "build the kernel only through `cargo xtask`";
  - the new policy line.

- [ ] **Step 1: Make the scenario fail.** In `boot` `must_contain`, after the EL0-interrupt line, add:
  - `"fault: reading kernel memory at 0x40080000"`;
  - `"fault[2]: unhandled exception: DABT (lower EL), ESR 0x"`;
  - `"level 3 permission fault"`;
  - `"pc : 0000000000400"`;
  - `"rust_kernel: fault[2] killed by signal 11 (SIGSEGV)"`;
  - `"undef: running an undefined instruction"`;
  - `"undef[3]: unhandled exception: undefined instruction"`;
  - `"rust_kernel: undef[3] killed by signal 4 (SIGILL)"`;
  - and `"rust_kernel: boot OK, powering off"` last.
- [ ] **Step 2: Run and confirm it fails.** Run `cargo xtask test`. Expected: `scenario boot: missing (in order): "fault: reading kernel memory at 0x40080000"`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run and confirm it passes.** Run `cargo xtask test`. Expected:
  - host tests, both policies, clippy and every scenario are `ok`;
  - `boot` shows three programs and `boot OK`.

  This meets the M3 exit test: a Rust user program prints from EL0 and exits; ticks are counted; a user fault is reported, not fatal.
- [ ] **Step 5: Update `README.md`** as listed above.
- [ ] **Step 6: Commit**

```bash
git add foundation kernel xtask README.md
git commit -m "feat(user): Linux-style user fault reports; a faulting program is killed and the boot goes on"
```

---

## After M3

1. **Close step.** Update `docs/study/03-exceptions.md` with what really happened:
   - the real `CNTFRQ` banner, the EL0-interrupt count, the user binary sizes and the final `SPIN`;
   - the rulings made while executing;
   - the fact that cache maintenance and the timer frequency were checked only on QEMU.
2. **Decide again before M4:**
   - per-task kernel stacks and an IRQ stack. The overflow check moves to them;
   - `UserPtr` with fault fix-ups, which replaces the window check;
   - an ELF loader and per-process address spaces, which replace the fixed window and the flat binaries;
   - DBM or dirty tracking for user pages;
   - moving the kernel image into the high half;
   - the Redox note's question 2 (a register check across preemption).
3. **M7** makes `KERNEL_CONTEXT`, `USER_EXIT`, the overflow stack and `FATAL` per CPU. It changes the one-writer loads and stores into real atomics where needed.
