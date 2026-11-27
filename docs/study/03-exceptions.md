# 03 — Exceptions, interrupts, the timer and the first system calls (ARM64)

**Linux reference:** v7.3 (`c03c1a14507d`), in `reference/linux`

**Scope:** the M3 path. An exception arrives at the vector table. Assembly saves the registers. C code reads `ESR_EL1` and picks a handler. That handler is a system call, a user fault, or an interrupt from the GIC, and the timer tick is one such interrupt. Signals, processes and page-fault handling with VMAs come later (M4, M6).

**Citations:** Linux paths are relative to `reference/linux`. Our paths (`foundation/…`, `klib/…`, `kernel/…`) are relative to the project root, after M2.

## Files read

- `arch/arm64/kernel/entry.S`:
  - `:33-37` (`clear_gp_regs`), `:39-101` (`kernel_ventry`: one 128-byte vector slot);
  - `:197-333` (`kernel_entry`: save the registers), `:335-457` (`kernel_exit`: restore them, `eret`);
  - `:512-533` (the `vectors` table), `:535-561` (`__bad_stack`);
  - `:564-598` (`entry_handler`: assembly → C), `:600-611` (`ret_to_kernel`, `ret_to_user`);
  - `:850-859` (`ret_from_fork`), `:868-897` (`call_on_irq_stack`).
- `arch/arm64/include/asm/ptrace.h:93` (`NO_SYSCALL`), `:154-170` (`struct pt_regs`), `:196-197` (`user_mode`); `arch/arm64/include/uapi/asm/ptrace.h:32-48` (PSTATE mode and DAIF bits).
- `arch/arm64/kernel/asm-offsets.c:72-81`: `pt_regs` field offsets for assembly.
- `arch/arm64/kernel/entry-common.c`:
  - `:172-183` (`do_interrupt_handler`), `:188-208` (`__panic_unhandled`, `UNHANDLED`);
  - `:335-344` (`el1_abort`), `:462-509` (`el1h_64_sync_handler`), `:535-549` (`el1_interrupt`);
  - `:568-594` (`el0_da`, `el0_ia`), `:689-695` (`el0_inv`), `:752-761` (`el0_svc`);
  - `:771-837` (`el0t_64_sync_handler`), `:839-864` (`el0_interrupt`, `el0t_64_irq_handler`).
- `arch/arm64/kernel/syscall.c:22-55` (`do_ni_syscall`, `invoke_syscall`), `:62-136` (`el0_svc_common`), `:138-141` (`do_el0_svc`).
- `arch/arm64/kernel/sys.c:39-63` (`sys_call_table`), `arch/arm64/include/asm/syscall_wrapper.h:49-70`, `arch/arm64/include/asm/syscall.h:51-62` (`syscall_set_return_value`).
- `scripts/syscall.tbl:83` (`write` = 64), `:120` (`exit` = 93), `:121` (`exit_group` = 94); `arch/arm64/kernel/Makefile.syscalls:1-6`; `include/uapi/asm-generic/unistd.h:871` (`__NR_syscalls` = 473).
- `fs/read_write.c:667-689` (`vfs_write`), `:747-751` (`sys_write`); `kernel/exit.c:1094-1097` (`sys_exit`), `:939-941` ("Attempted to kill init!").
- `include/asm-generic/access_ok.h:31-45`, `arch/arm64/include/asm/uaccess.h:29-50` (`access_ok`).
- `arch/arm64/include/asm/esr.h:13-115` (exception classes and fields); `arch/arm64/kernel/traps.c:832-880` (`esr_class_str`).
- `arch/arm64/mm/fault.c:99-118` (`mem_abort_decode`), `:345-360` (`die_kernel_fault`), `:518-534` (`do_bad_area`), `:917-934` (`fault_info`), `:984-1001` (`do_mem_abort`).
- `arch/arm64/kernel/traps.c:150` (`show_unhandled_signals`), `:237-269` (`arm64_show_signal`, `arm64_force_sig_fault`), `:889-898` (`bad_el0_sync`); `kernel/signal.c:542-548` (`unhandled_signal`).
- `arch/arm64/kernel/irq.c:97-105` (`handle_arch_irq`, `set_handle_irq`).
- `drivers/irqchip/irq-gic.c`:
  - `:179-226` (mask, unmask, EOI), `:334-372` (`gic_handle_irq`);
  - `:440-525` (`gic_cpu_if_up`, `gic_dist_init`, `gic_cpu_init`);
  - `:1081-1101` (DT cells → hardware IRQ), `:1287-1295` (`gic_check_eoimode`), `:1490-1491`, `:1515-1523` (DT `compatible` strings).
- `drivers/irqchip/irq-gic-common.c:91-142`; `include/linux/irqchip/arm-gic.h:10-62`; `include/linux/irqchip/arm-gic-common.h:12`.
- `kernel/irq/chip.c:904-943` (`handle_percpu_devid_irq`).
- `drivers/clocksource/arm_arch_timer.c`:
  - `:560-581` (`timer_handler`), `:612-629` (`set_next_event`), `:670-709` (`__arch_timer_setup`);
  - `:823-842` (`arch_timer_starting_cpu`), `:874-879` (`arch_timer_banner`);
  - `:1132-1150` (`arch_timer_select_ppi`), `:1172-1179` (reading the DT).
- `include/clocksource/arm_arch_timer.h:12-14, 35-38`; `arch/arm64/include/asm/arch_timer.h:117-156`.
- `kernel/time/tick-common.c:86-147` (`tick_periodic`, `tick_handle_periodic`); `kernel/time/timekeeping.c:2828-2831` (`do_timer`); `kernel/Kconfig.hz:8`.
- `arch/arm64/include/asm/irqflags.h:23-57, 163-197`; `arch/arm64/include/asm/daifflags.h:15-18`.
- `arch/arm64/include/asm/processor.h:305-345` (`start_thread`).
- `tools/include/nolibc/arch-arm64.h:25-36, 146-152`: Linux's own tiny user-space syscall stubs.
- `Documentation/devicetree/bindings/interrupt-controller/arm,gic.yaml:75-90`, `Documentation/devicetree/bindings/timer/arm,arch_timer.yaml:31-37`.

## How Linux does it

```
EL0 or EL1 ──exception──► VBAR_EL1 + offset           (hardware: one of 16 slots)
                              │
              kernel_ventry: reserve pt_regs, check for stack overflow
                              │
              kernel_entry: save x0–x30, sp, pc (ELR), pstate (SPSR)
                              │
              el0t_64_sync_handler(regs), el1h_64_irq_handler(regs), …   (C)
                 │ switch on ESR_EL1.EC
                 ├─ SVC64 ──► el0_svc ──► invoke_syscall: sys_call_table[x8]
                 ├─ DABT_LOW / IABT_LOW ──► do_mem_abort ──► fault_info[FSC]
                 │                              └─ no mapping: signal (SIGSEGV)
                 └─ IRQ ──► handle_arch_irq = gic_handle_irq
                                 └─ read IAR ► timer handler ► write EOI
                              │
              kernel_exit: restore registers, eret        (back to EL0 or EL1)
```

### Step 1: the vector table

1. **Sixteen slots of 128 bytes each.** The table is 2 KiB-aligned (`.align 11`, `entry.S:512`). There are four groups: EL1 using `SP_EL0` ("t"), EL1 using `SP_EL1` ("h"), 64-bit EL0, and 32-bit EL0. Each group has sync, IRQ, FIQ and SError (`entry.S:513-533`).
2. **One slot** (`kernel_ventry`, `entry.S:39-101`):
   - It reserves a `pt_regs` frame on the stack: `sub sp, sp, #PT_REGS_SIZE` (`:57`).
   - It checks for stack overflow **without using a spare register**. It adds `x0` to `sp` and subtracts it again, so one register holds two values for a moment (`:63-67`). Kernel stacks are aligned so that bit `THREAD_SHIFT` of a valid `sp` is always 0. `tbnz x0, #THREAD_SHIFT` (`:65`) jumps to the overflow stack and to `__bad_stack` (`:535-561`).
   - `.org .Lventry_start + 128` (`:100`) makes the **assembler** fail if a slot grows past 128 bytes.
3. **`entry_handler`** (`entry.S:564-575`) joins assembly to C. It runs `kernel_entry`, then `mov x0, sp` (a pointer to the frame) and `bl el0t_64_sync_handler` (or the matching name). Afterwards it jumps to `ret_to_user` or `ret_to_kernel` (`:600-611`).

### Step 2: saving and restoring the registers

- **`kernel_entry`** (`entry.S:197-333`):
  - It stores `x0`–`x29` in pairs (`:204-218`).
  - From EL0, it then **zeroes all general registers** (`clear_gp_regs`, `:221`), so user values cannot steer speculation in the kernel. It saves the user stack pointer from `sp_el0` (`:222`). While the kernel runs, `sp_el0` holds the `current` task pointer (`:224`).
  - It reads `ELR_EL1` (the return address) and `SPSR_EL1` (the saved PSTATE) (`:281-282`) and stores them as `pc` and `pstate` (`:304`).
  - From EL0, it sets `syscallno = NO_SYSCALL` (`:308`). Only a real `svc` changes it.
- **`kernel_exit`** (`entry.S:335-457`) does the reverse. Back to EL0, it puts the saved user `sp` into `sp_el0` (`:361-362`). It writes `ELR_EL1` and `SPSR_EL1` (`:406-407`), reloads `x0`–`x30`, frees the frame, and executes `eret` (`:455`). The `sb` after `eret` (`:456`) stops the CPU from running past `eret` speculatively.
- **`struct pt_regs`** (`ptrace.h:154-170`) is the frame: `regs[31]`, `sp`, `pc`, `pstate`, then `orig_x0`, `syscallno`, and a few more fields. It must be a multiple of 16 bytes (`:173`), because `sp` must stay 16-byte aligned.
- **`asm-offsets.c:72-81`** makes the C compiler compute `S_PC`, `S_SP`, `PT_REGS_SIZE` and the other offsets for the assembly. The assembly and the C struct can never disagree.

### Step 3: dispatch on `ESR_EL1`

- `ESR_EL1` bits 31:26 are the **exception class** (EC) (`esr.h:73-76`). The classes M3 needs:

| EC | Name | Text (from `esr_class_str`) | Meaning |
|---|---|---|---|
| `0x00` | `UNKNOWN` (`esr.h:13`) | "Unknown/Uncategorized" | undefined instruction |
| `0x07` | `FP_ASIMD` (`esr.h:20`) | — | FP/SIMD used while trapped |
| `0x15` | `SVC64` (`esr.h:33`) | "SVC (AArch64)" | `svc` from 64-bit EL0 |
| `0x20` | `IABT_LOW` (`esr.h:44`) | "IABT (lower EL)" | instruction fetch fault from EL0 |
| `0x24` | `DABT_LOW` (`esr.h:48`) | "DABT (lower EL)" | data access fault from EL0 |
| `0x25` | `DABT_CUR` (`esr.h:49`) | "DABT (current EL)" | data access fault in the kernel |
| `0x3C` | `BRK64` (`esr.h:69`) | — | `brk` instruction |

- **`el0t_64_sync_handler`** (`entry-common.c:771-837`) is one `switch` on EC: `SVC64` → `el0_svc` (`:776`), `DABT_LOW` → `el0_da` (`:779`), and so on. Anything unknown goes to `el0_inv` (`:834-835`), which sends `SIGILL` through `bad_el0_sync` (`traps.c:889-898`). **A bad user exception never stops the kernel.**
- **`el1h_64_sync_handler`** (`entry-common.c:462-509`) handles kernel faults. Data and instruction aborts go to `do_mem_abort` (`el1_abort`, `:335-344`). Anything unknown is fatal: `__panic_unhandled` (`:188-201`) prints `Unhandled %s exception on CPU%d, ESR 0x%016lx -- %s` and panics.
- **Never expected at all.** EL1t (kernel on `SP_EL0`) and every 32-bit EL0 vector use the `UNHANDLED` macro (`:203-208`, used at `:330-333` and `:986-989`).
- **Interrupts are masked or unmasked on purpose.** A user sync handler first runs `local_daif_restore(DAIF_PROCCTX)` (`daifflags.h:15`, value 0), so IRQs are on while a system call runs (`entry-common.c:757`). An interrupt handler instead writes `DAIF_PROCCTX_NOIRQ` (`:538`, `:844`): debug and SError exceptions are on, IRQ and FIQ stay off.

### Step 4: system calls

1. **The ABI.** The user puts the number in `x8` and up to six arguments in `x0`–`x5`, then runs `svc #0`. The result comes back in `x0`. Linux's own tiny libc shows the user side: `register long _num __asm__ ("x8")`, then `svc #0` (`tools/include/nolibc/arch-arm64.h:25-36`).
2. **`do_el0_svc`** (`syscall.c:138-141`) passes `regs->regs[8]`, `__NR_syscalls` and `sys_call_table` to `el0_svc_common`.
3. **`el0_svc_common`** (`syscall.c:62-136`) saves `orig_x0` and `syscallno` (`:67-68`), because `x0` will be overwritten by the result. Then it handles tracing and calls `invoke_syscall`.
4. **`invoke_syscall`** (`syscall.c:38-55`):
   - `if (scno < sc_nr)`: a **bounds check** before the table is indexed (`:46`).
   - `array_index_nospec` (`:48`): the index also stays in range during speculation.
   - Out of range: `do_ni_syscall` (`:22-31`), which returns `-ENOSYS`.
   - The result is stored with `syscall_set_return_value` (`syscall.h:51-62`): `regs->regs[0] = val`. An error is a negative number in `x0`.
5. **The table.** `sys_call_table` starts with **every** entry set to `__arm64_sys_ni_syscall` (`sys.c:61`, `[0 ... __NR_syscalls - 1]`). The generated list from `syscall.tbl` then overrides the entries that exist (`sys.c:62`). A missing number can never be a null pointer. arm64 uses `scripts/syscall.tbl` directly: `arch/arm64/tools/syscall_64.tbl` is a symlink to it, and `Makefile.syscalls:1-6` adds the ABIs `renameat rlimit memfd_secret`.
6. **Typed arguments.** `SYSCALL_DEFINE3(write, unsigned int, fd, const char __user *, buf, size_t, count)` (`fs/read_write.c:747`) expands to `__arm64_sys_write(const struct pt_regs *regs)`. That wrapper unpacks `regs->orig_x0`, `regs->regs[1]` and the other registers into typed arguments (`syscall_wrapper.h:49-70`). The system call itself never sees `pt_regs`.
7. **The two calls of M3:**
   - `write` = 64 (`syscall.tbl:83`). `vfs_write` checks `access_ok(buf, count)` and returns `-EFAULT` if it fails (`read_write.c:675-676`). On arm64, `access_ok` means `addr + size <= TASK_SIZE_MAX`, computed as if with 65 bits so it cannot overflow (`uaccess.h:29-50`, generic version at `access_ok.h:31-41`). This works because **the kernel lives only in the upper half** (TTBR1), so every lower address is a user address.
   - `exit` = 93 (`syscall.tbl:120`): `do_exit((error_code & 0xff) << 8)` (`kernel/exit.c:1094-1097`). Only the low 8 bits of the exit code survive. `exit_group` = 94 (`syscall.tbl:121`) is what libc's `exit()` calls.

### Step 5: user faults

1. `el0_da` and `el0_ia` (`entry-common.c:568-594`) read `FAR_EL1` (the faulting address) and call `do_mem_abort(far, esr, regs)`.
2. `do_mem_abort` (`fault.c:984-1001`) indexes `fault_info[]` (`:917-934`) with the fault status code `ESR.FSC` (bits 5:0). Each entry holds a handler, a signal, a code and a name. Examples: `"level 3 translation fault"` → `do_translation_fault`, `SIGSEGV`; `"level 3 permission fault"` → `do_page_fault`, `SIGSEGV`.
3. There is no VMA for the address, so the path ends in `do_bad_area` (`fault.c:518-534`) or `bad_area:` (`:781`). From user mode, both call `arm64_force_sig_fault(SIGSEGV, …, inf->name)` (`:832`).
4. `arm64_show_signal` (`traps.c:237-259`) prints one line and a register dump:
   `<comm>[<pid>]: unhandled exception: DABT (lower EL), ESR 0x…, level 3 translation fault in <vma>`.
   - **By default it prints nothing.** `show_unhandled_signals` is 0 on arm64 (`traps.c:150`). The sysctl `debug.exception-trace` turns it on (`kernel/signal.c:5048`).
5. The signal's default action kills the task. **If the task is `init` (pid 1), the kernel panics:** `Attempted to kill init! exitcode=0x%08x` (`kernel/exit.c:939-941`).
6. **A kernel fault is different:** `die_kernel_fault` (`fault.c:345-360`) prints `Unable to handle kernel %s at virtual address %016lx` and a decode of the ESR (`mem_abort_decode`, `:99-118`), then "Oops".

### Step 6: the GIC (QEMU `virt` has a GICv2)

**What QEMU passes us** (`qemu-system-aarch64` 11.1.3, `-machine virt -cpu cortex-a72 -m 512M`, dumped with `dumpdtb` and decoded with `dtc`):

| Node | Property | Value | Meaning |
|---|---|---|---|
| `/intc@8000000` | `compatible` | `"arm,cortex-a15-gic"` | GICv2. Linux's `irq-gic.c` claims it (`IRQCHIP_DECLARE`, `irq-gic.c:1518`), so `irq-gic-v3.c` is not needed. |
| | `reg` | `<0 0x8000000 0 0x10000>`, `<0 0x8010000 0 0x10000>` | distributor (GICD) at `0x0800_0000`, CPU interface (GICC) at `0x0801_0000`, 64 KiB each. Linux maps index 0 and index 1 (`irq-gic.c:1394`, `:1398`). |
| | `#interrupt-cells` | 3 | type, number, flags (`arm,gic.yaml:75-90`) |
| `/timer` | `compatible` | `"arm,armv8-timer"` | the per-CPU architected timer |
| | `interrupts` | `<1 13 0x104>`, `<1 14 0x104>`, `<1 11 0x104>`, `<1 10 0x104>` | four PPIs (type 1), in the binding's order: secure physical, non-secure physical, **virtual**, EL2 physical (`arm,arch_timer.yaml:33-36`). Flags `0x104`: bits 15:8 = `0x01`, the CPU mask (CPU 0); bits 3:0 = 4, active-high level. |

- **PPI number → interrupt ID.** The GIC driver adds 16 to a PPI number (`irq-gic.c:1100-1101`). So the virtual timer is PPI 11 = **INTID 27**. IDs 0–15 are SGIs (software interrupts between CPUs), 16–31 are PPIs (per CPU), and 32 and up are SPIs (shared).
- **Registers** (`arm-gic.h`):
  - Distributor: `CTRL` `0x000`, `ENABLE_SET` `0x100`, `ENABLE_CLEAR` `0x180`, `ACTIVE_CLEAR` `0x380`, `PRI` `0x400`, `TARGET` `0x800`, `CONFIG` `0xc00` (`:42-54`).
  - CPU interface: `CTRL` `0x00`, `PRIMASK` `0x04`, `INTACK` `0x0c`, `EOI` `0x10` (`:10-14`).
- **Start-up:**
  1. `gic_dist_init` (`irq-gic.c:464-485`): disable the distributor. Route every SPI to this CPU. Set SPIs to level-triggered, priority `0xa0` (`GICD_INT_DEF_PRI`, `arm-gic-common.h:12`), deactivated and disabled (`gic_dist_config`, `irq-gic-common.c:91-119`). Enable the distributor.
  2. `gic_cpu_init` (`irq-gic.c:487-525`): disable and deactivate this CPU's SGIs and PPIs, and give them priority `0xa0` (`gic_cpu_config`, `irq-gic-common.c:121-142`). Set the priority mask to `0xf0` (`GICC_INT_PRI_THRESHOLD`, `irq-gic.c:521`). An interrupt is signalled only when its priority value is lower than the mask, and `0xa0 < 0xf0`.
  3. `gic_cpu_if_up` (`irq-gic.c:440-461`): enable the CPU interface. It keeps the bypass bits and does not set `EOImodeNS` in our case (see below).
  4. `set_handle_irq(gic_handle_irq)` (`irq-gic.c:1255`): the arch IRQ entry now calls the GIC (`arch/arm64/kernel/irq.c:97-105`). This is a function pointer, set once at boot.
- **One interrupt** (`gic_handle_irq`, `irq-gic.c:334-372`):
  1. Read `GICC_IAR` (`INTACK`). The low 10 bits are the ID (`GICC_IAR_INT_ID_MASK`, `arm-gic.h:38`). Reading it also marks the interrupt "active".
  2. IDs of 1020 and up mean "nothing pending", so the loop stops (`irq-gic.c:344-345`).
  3. Otherwise call `generic_handle_domain_irq` (`irq-gic.c:370`), and loop, because another interrupt may be pending already.
- **Where the EOI happens in our case.** We boot at EL1, so hypervisor mode is not available. `gic_check_eoimode` returns false at once (`irq-gic.c:1293-1294`). Linux then disables `supports_deactivate_key` (`:1490-1491`):
  - The early EOI write inside `gic_handle_irq` (`irq-gic.c:347-348`) is skipped.
  - The timer is a per-CPU interrupt, so its flow handler is `handle_percpu_devid_irq` (`kernel/irq/chip.c:904`). It calls the timer's handler (`:927`) **and then** `chip->irq_eoi` (`:942-943`).
  - `gic_eoi_irq` (`irq-gic.c:218-226`) writes the ID to `GICC_EOIR`. This one write both drops the running priority and deactivates the interrupt.

### Step 7: the architected timer and the tick

- **Which timer.** `arch_timer_select_ppi` (`arm_arch_timer.c:1132-1150`): when the kernel is not in hyp mode and hyp mode is not available, Linux uses the **virtual** timer (`:1143-1144`). That means the `CNTV_*` registers (`arch_timer.h:117-121`, `:144`). The PPIs are read from the DT in the binding's order (`arm_arch_timer.c:1172-1179`, enum order at `arm_arch_timer.h:35-38`).
- **The frequency** comes from `CNTFRQ_EL0` (`arch_timer.h:156`), which firmware sets. QEMU sets it too. Linux prints it: `cp15 timer running at %lu.%02luMHz (%s).` (`arm_arch_timer.c:874-879`).
- **Control bits** (`arm_arch_timer.h:12-14`): `ENABLE` (bit 0), `IT_MASK` (bit 1), `IT_STAT` (bit 2, read-only: the condition is met).
- **Programming one event** (`set_next_event`, `arm_arch_timer.c:612-629`): read the counter (`CNTVCT_EL0`). Write `CVAL = now + delta`. Write `CTRL` with `ENABLE` set and `IT_MASK` clear.
- **Handling it** (`timer_handler`, `arm_arch_timer.c:560-574`): if `IT_STAT` is set, **set `IT_MASK` first**, then call the clock-event handler. The timer interrupt is level-triggered. Until it is masked or `CVAL` moves forward, the line stays high, and the interrupt would fire again right after the EOI.
- **A periodic tick from a one-shot timer.** The device only says `CLOCK_EVT_FEAT_ONESHOT` (`arm_arch_timer.c:675`). `tick_handle_periodic` (`kernel/time/tick-common.c:108-147`) calls `tick_periodic`, then programs the next event at `next = next + TICK_NSEC`, **based on the previous deadline, not on "now"**. A late interrupt therefore causes no drift. `tick_periodic` (`:86-103`) calls `do_timer(1)`, which does `jiffies_64 += ticks` (`kernel/time/timekeeping.c:2828-2831`).
- **HZ.** The default is `HZ_250` (`kernel/Kconfig.hz:8`), so there are 250 ticks per second.
- **Per-CPU start.** `arch_timer_starting_cpu` (`arm_arch_timer.c:823-842`) sets up the clock-event device and enables the PPI on this CPU's GIC interface.

### Step 8: masking interrupts

- `local_irq_disable()` is `msr daifset, #3`, and enable is `msr daifclr, #3` (`irqflags.h:23-57`). The immediate `#3` means the I and F bits: IRQ and FIQ.
- `arch_local_irq_save` reads `DAIF` and then masks. `arch_local_irq_restore` writes the saved value back (`irqflags.h:163-197`). Restoring, rather than enabling, makes nesting safe: inner code never turns interrupts on early.
- The `barrier()` calls around each `msr` stop the compiler from moving memory accesses across the mask change.

### Step 9: the first entry to EL0

- Linux never "calls" user code. `execve` fills the task's `pt_regs` with `start_thread` (`processor.h:339-345`): all registers zero, `pc` = the entry point, `pstate = PSR_MODE_EL0t` (0, `uapi/asm/ptrace.h:32`: EL0, interrupts unmasked), and `sp` = the user stack.
- Then the normal exit path runs: `ret_to_user` → `kernel_exit 0` → `eret` (`entry.S:604-611`). A brand-new task gets there through `ret_from_fork` (`:850-859`). **Entering user mode is just returning from an exception that never happened.**

## Key data structures

- **`struct pt_regs`** (`ptrace.h:154-170`): it lives on the task's kernel stack, at the top. `kernel_entry` builds it, and `kernel_exit` consumes it. Its first part (`user_pt_regs`) is the layout `ptrace` shows to user space.
- **`sys_call_table[__NR_syscalls]`** (`sys.c:60-63`): 473 function pointers (`unistd.h:871`). It is `const`, so it lives in read-only memory.
- **`fault_info[64]`** (`fault.c:917`): one entry per FSC value, giving the handler, signal, code and name.
- **`esr_class_str[]`** (`traps.c:832`): one name per EC value, for messages.
- **`gic_data[]`** (`irq-gic.c`): the GIC's base addresses and IRQ domain. Set up once at boot.
- **`struct clock_event_device`**: the timer seen by the time core. It holds `set_next_event`, the shutdown functions and `event_handler`, which is `tick_handle_periodic` for a periodic tick.

## Engineering techniques worth stealing

- **Let the compiler compute the frame layout for the assembly** (`asm-offsets.c`). In Rust: `#[repr(C)] struct PtRegs`, then `core::mem::offset_of!` passed into `global_asm!` as `const` operands, plus a compile-time assert on the size.
- **Make the assembler check the slot size** (`.org … + 128`, `entry.S:100`). `global_asm!` takes the same directive.
- **A table with a safe default in every slot** (`sys.c:61`), plus a bounds check before indexing (`syscall.c:46`). In Rust, a `match` with a `_ => -ENOSYS` arm gives both for free.
- **Keep the raw frame away from the system call.** `SYSCALL_DEFINEx` unpacks `pt_regs` into typed arguments, so `sys_write` sees `fd`, `buf` and `count`, never registers.
- **Tables indexed by hardware codes** (`fault_info[FSC]`, `esr_class_str[EC]`) turn numbers into the text of a message. Easy to port as `const` arrays in `klib`, and easy to unit-test on the Mac.
- **Zero the user's registers on entry** (`clear_gp_regs`), so user values cannot steer kernel speculation.
- **Mask the level-triggered timer before the EOI**, or reprogram it first (`timer_handler`).
- **Program the next tick from the last deadline, not from "now"** (`tick_handle_periodic`): no drift.
- **Loop on `IAR` until it reads spurious** (`gic_handle_irq`): one exception can serve several interrupts.
- **Report a user fault and keep running.** A bad user exception becomes a signal, never a kernel panic (`el0_inv`, `do_bad_area`).
- **Entering user mode is an `eret` from a prepared frame** (`start_thread` + `ret_to_user`). The same code path serves the first entry and every system-call return.

## Our Rust design and why it differs (proposal for the M3 plan)

What M2 left that M3 must change:
- The early vectors print `ESR`, `FAR` and `ELR`, then power off. They save no registers and cannot return (`foundation/src/arch/aarch64/vectors.rs:15-28`, `:77-93`). Their comment cites `entry.S:514-529`; the table is really at `entry.S:513-533`.
- The page tables cannot map user pages. `leaf_descriptor` always sets UXN (`klib/src/arch/aarch64/pgtable.rs:118`), and `Access` has no EL0 variant (`klib/src/mm/map.rs:11-15`).
- `boot_mappings` maps only the UART as a device (`klib/src/mm/layout.rs:101-134`). The GIC needs two more Device regions.
- `SpinLock` does not mask interrupts (`foundation/src/sync/spinlock.rs:11-12`). `printk` takes no lock (`kernel/src/kernel/printk.rs:11`), so printing from an interrupt cannot deadlock. Output lines could still mix, though.
- There is one boot stack of 64 KiB, with no guard page (`foundation/src/arch/aarch64/kernel.ld:45-48`).

| Linux | Ours (proposal) | Why |
|---|---|---|
| `entry.S`: `kernel_ventry`, `kernel_entry`, `kernel_exit` | `global_asm!` in `foundation/src/arch/aarch64/entry.rs`: the same 16 slots and the same `pt_regs` save and restore. Offsets come from `offset_of!` on a `#[repr(C)] PtRegs`. | Pure Rust, no `.S` files. The layout is checked by the compiler, as `asm-offsets.c` does for Linux. |
| Overflow check with `THREAD_SHIFT` alignment, overflow stack | Not in M3. One kernel stack, no per-task stacks yet. A guard page below the boot stack is cheap: a stack overflow then faults instead of silently overwriting `.bss`. | The alignment trick needs per-task stacks (M4). |
| `el0t_64_sync_handler`: C `switch` on EC | `foundation` decodes `ESR_EL1` into a Rust `enum` (`Svc`, `DataAbort { far, fsc, write }`, `InstrAbort`, `Undefined`, `Other(ec)`). Safe code in `kernel` matches on it. The decoder lives in `klib` and is tested on the Mac. | `match` must cover every case, so no exception class is forgotten. |
| `handle_arch_irq` function pointer, set once (`set_handle_irq`) | The same idea: `kernel` registers its handlers with `foundation` once at boot. Plain `fn` items, no `#[no_mangle]`. | `kernel` forbids `unsafe`, and `#[unsafe(no_mangle)]` counts as unsafe. |
| `sys_call_table[]` + `SYSCALL_DEFINEx` | Generic code in `kernel` (ARM64 and RISC-V share `scripts/syscall.tbl`): `match nr { 64 => sys_write(fd, buf, count), 93 => sys_exit(code), _ => -ENOSYS }`. Each `sys_*` gets typed arguments. Unknown numbers are logged once. | The same ABI. A `match` has no null entries and needs no bounds check. M6 adds names from `syscall.tbl`. |
| `access_ok`: `addr + size <= TASK_SIZE` | **Not enough for us.** Our kernel image is identity-mapped in the low half (TTBR0), at `0x4008_0000`. "Below `TASK_SIZE`" would accept kernel addresses. M3 checks that the range lies inside the user program's own mapped window. | Linux puts the kernel only in TTBR1. We move the image high in M4 or later (see the Redox note). |
| `copy_from_user` with exception-table fix-ups | A small `copy_from_user` in `foundation`: check the window, then copy into a bounded kernel buffer. The console writes from kernel-owned bytes. No fix-ups are needed: the window is checked to be mapped. | This is the Redox note's question 1. The full `UserPtr` and fault fix-ups wait for M4's address spaces. |
| User fault → `SIGSEGV`; printed only with `debug.exception-trace=1` (default 0, `traps.c:150`); a fault in `init` panics the kernel | **Always print** Linux's line: `hello[1]: unhandled exception: DABT (lower EL), ESR 0x…, level 3 translation fault`, plus `pc`, `sp` and `far`. Then the program is killed, and the kernel goes on. | The exit test wants "reported, not fatal". We have no signals yet (M6) and no `init` rule yet. |
| Kernel fault: `die_kernel_fault` → Oops | `Unable to handle kernel paging request at virtual address …`, an ESR decode, then our panic and power-off. | The same message. We have no Oops recovery. |
| `__panic_unhandled` for EL1t and 32-bit EL0 | The same: these vectors print `Unhandled … exception` and panic. | There is no 32-bit user mode. |
| GICv2 driver, IRQ domains, flow handlers | `foundation/src/drivers/irqchip/gic.rs`: init as Linux does, read `IAR`, write `EOIR`. The base addresses come from the DT (`arm,cortex-a15-gic`). A tiny fixed table maps an INTID to a handler. No IRQ domains. | One interrupt source in M3. EOI mode 0, the same as Linux at EL1. |
| arch timer: virtual timer, one-shot device, periodic tick emulated | The virtual timer (`CNTV_*`), PPI 11 → INTID 27, read from the DT. `HZ = 250`. Next deadline = last deadline + `CNTFRQ / HZ`. `jiffies` is an `AtomicU64`. Print `CNTFRQ` as Linux's banner does. | The same behaviour, without the clock-event framework. |
| `local_irq_save/restore` with DAIF | `foundation::arch::irq_save() -> IrqFlags` and `irq_restore(IrqFlags)`. `IrqFlags` is not `Copy`, so a saved state is restored once. Add `SpinLock::lock_irqsave`, as M2 noted. | A lock used by an interrupt handler must be taken with IRQs masked, or one CPU can deadlock on itself. |
| `start_thread` + `ret_to_user` | `foundation` builds a `PtRegs` with `pc`, `sp` and `pstate = EL0t`, then leaves through the same `kernel_exit` code. | One exit path, as in Linux. |
| `/init` loaded by `execve` from a filesystem | A small Rust user program, built separately and embedded in the kernel image. It is copied into user pages at a fixed low address such as `0x40_0000`. | We have no ELF loader (M4) and no filesystem (M5). |
| User code may use FP/SIMD (Linux enables it lazily) | Build the user program for `aarch64-unknown-none-softfloat` too. | We never enable FP at EL0. An FP instruction would trap as `FP_ASIMD`. |

## Open questions (decisions for the M3 plan)

1. **How do we build and load the first user program?**
   - **A (recommended).** A new small crate, for example `user/hello`. `xtask` builds it, links it at `0x40_0000` and turns it into a flat binary. The kernel embeds the bytes with `include_bytes!` and copies them into user pages.
   - **B.** Put the user code in a special section of the kernel image, mapped for EL0. This is simpler, but user and kernel code mix, and M4 cannot reuse it.
   - **C.** Write a small ELF loader now. This brings M4 work forward.
2. **The `unsafe` rule and the user program.** A user program needs `asm!("svc #0")`, and that is `unsafe`. Today only `foundation` (and the `vmlinux` glue line) may use `unsafe`.
   - **A (recommended).** Allow `unsafe` in one small file of the user crate: the `svc` stubs, like Linux's `nolibc` (`arch-arm64.h`). It must have `SAFETY:` comments. `cargo xtask test` checks that no other user file uses `unsafe`.
   - **B.** Write the user program in `global_asm!` only. Then it is not really a Rust user program, and the exit test asks for one.
3. **What happens when the user program ends, by `exit` or by a fault?** Linux never returns to the caller: the scheduler runs another task. We have no scheduler yet.
   - **A (recommended).** `run_user()` returns to `start_kernel` with the result: "exited with code N" or "killed by SIGSEGV". To do this, `foundation` saves a small kernel context before `eret` and resumes it later. This is good practice for M4's `cpu_switch_to`. The kernel prints the result, runs the next test, and powers off cleanly.
   - **B.** `exit` powers off directly. The fault test then needs its own boot. This is simpler, but it is not how Linux behaves.
4. **How does `write` read the user's buffer?** (The Redox note's question 1.)
   - **A (recommended).** A small `copy_from_user` in `foundation` now. It checks that the range is inside the user window, not just "below `TASK_SIZE`", because our kernel is in the low half too. Then it copies into a bounded kernel buffer.
   - **B.** Bring M4's full `UserPtr` interface forward, with per-process address spaces.
5. **The tick rate, and how we test it.**
   - **A (recommended).** `HZ = 250`, Linux's default, with a periodic tick only (no tickless mode). Test: enable IRQs, wait with `wfi` until `jiffies` reaches 25 (0.1 s), then print the count.
   - **B.** `HZ = 100`. That means fewer interrupts under QEMU's software CPU model, but it differs from Linux's default.

**Smaller choices the plan will make unless you object:**
- Interrupts run on the current kernel stack. Linux switches to a per-CPU IRQ stack (`call_on_irq_stack`, `entry.S:868-897`). We add one with per-task stacks in M4.
- Add a guard page below the 64 KiB boot stack.
- Fix the `entry.S:514-529` citation in `vectors.rs` when that file is replaced.
- The Redox note's question 2 (checking registers across preemption) is for M4, not M3.
