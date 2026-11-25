# 01 — How Linux boots on ARM64

**Linux reference:** v7.3 (`c03c1a14507d`), in `reference/linux`

## Files read

- `Documentation/arch/arm64/booting.rst:48-60`: rules for the device tree (8-byte aligned, at most 2 MB).
- `Documentation/arch/arm64/booting.rst:73-160`: the 64-byte Image header and where the image must be placed.
- `Documentation/arch/arm64/booting.rst:161-200`: CPU state at entry (`x0` = DTB, all interrupts masked, MMU off, EL2 or EL1).
- `arch/arm64/kernel/head.S:42-70`: the real Image header.
- `arch/arm64/kernel/head.S:85-131` (`primary_entry`), `:220-247` (`__primary_switched`) and `:270` (`init_kernel_el`).
- `arch/arm64/kernel/image.h:54-65`: how `image_size` and `flags` are computed at link time.
- `arch/arm64/kernel/vmlinux.lds.S:174-373`: section layout from `_text` to `_end`.
- `arch/arm64/kernel/pi/map_kernel.c:251-252`: where BSS is cleared.
- `init/main.c:987-1020`: the first calls in `start_kernel()`.
- `drivers/tty/serial/amba-pl011.c:2473-2481` (`pl011_console_putchar`) and `include/linux/amba/serial.h:25,34,91`.
- `drivers/tty/serial/serial_core.c:2120-2131` (`uart_console_write`).
- `kernel/panic.c:577` (`vpanic`), `:640` and `:757` (the panic banner lines).
- `drivers/firmware/psci/psci.c:287-293` (`get_set_conduit_method`) and `:332-335` (`psci_sys_poweroff`).
- `scripts/dtc/libfdt/fdt.c:89-140` (`fdt_check_header`).

## How Linux does it

1. **The bootloader** loads `Image` at a 2 MB-aligned base plus `text_offset`, puts the device tree somewhere in RAM, sets `x0 = DTB address`, and jumps to the first byte of the image (`booting.rst:136-170`). In QEMU, `-kernel Image` plays the bootloader.
2. **Header word 0** (`head.S:58`) is `efi_signature_nop`. It does nothing when booted normally, but it also reads as "MZ", which lets UEFI firmware treat the same file as a PE/COFF program. **Header word 1** is `b primary_entry`.
3. **`primary_entry`** (`head.S:85`):
   1. `record_mmu_state`: was the MMU on? The result goes in `x19`.
   2. `preserve_boot_args`: save `x0..x3`. The FDT pointer goes in `x21` (`head.S:78-83`), the first thing done, before anything can clobber `x0`.
   3. Set up an early stack, then build an identity map (`__pi_create_init_idmap`).
   4. `init_kernel_el` (`head.S:270`): if the CPU entered at EL2, configure EL2 and drop to EL1 (or stay in EL2 with VHE).
   5. `__cpu_setup`, then `__primary_switch`, which turns on the MMU (`__enable_mmu`, `head.S:511`).
4. **The early C code** in `pi/map_kernel.c` maps the kernel and clears BSS with `memset(__bss_start, 0, init_pg_end - __bss_start)` (`map_kernel.c:252`).
5. **`__primary_switched`** (`head.S:220`):
   1. Set `current` = `init_task`.
   2. Install the exception vectors (`vbar_el1`).
   3. Store the FDT pointer in `__fdt_pointer` (`head.S:231`).
   4. `bl start_kernel` (`head.S:246`). It must never return; `ASM_BUG()` follows.
6. **`start_kernel()`** (`init/main.c:987`) is the table of contents for the rest of the boot. The first calls, mapped to our milestones:

| Call (`init/main.c`) | What it does | Our milestone |
|---|---|---|
| `set_task_stack_end_magic(&init_task)` (992) | stack-overflow canary for task 0 | M4 |
| `smp_setup_processor_id()` (993) | which CPU are we | M7 |
| `debug_objects_early_init()` (994) | debug tracking | not planned |
| `init_vmlinux_build_id()` (995) | build ID for crash dumps | not planned |
| `cgroup_init_early()` (997) | control groups | not planned |
| `local_irq_disable()` (999) | keep interrupts off during early boot | M3 |
| `boot_cpu_init()` (1006) | mark CPU 0 online | M7 |
| `page_address_init()` (1007) | high memory (not used on 64-bit) | — |
| `pr_notice("%s", linux_banner)` (1008) | **the first message printed** | **M1** |
| `setup_arch(&command_line)` (1009) | arch setup: parses the DTB, memblock, paging | M2 |
| `mm_core_init_early()` (1010) | early memory core | M2 |
| `jump_label_init()` / `static_call_init()` (1012-13) | self-patching code | not planned |
| `setup_command_line()` (1016) | kernel command line | M2 |
| `setup_nr_cpu_ids()` / `setup_per_cpu_areas()` (1017-18) | per-CPU data | M7 |
| `smp_prepare_boot_cpu()` (1019) | arch-specific boot-CPU hooks | M7 |

7. **Console output.** `uart_console_write` (`serial_core.c:2120`) writes `'\r'` before every `'\n'`, so a serial terminal returns to column 0. `pl011_console_putchar` spins while `UART01x_FR_TXFF` (the TX FIFO is full, `FR` bit 5) and then writes the byte to `UART01x_DR`.
8. **Panic.** `vpanic` prints `Kernel panic - not syncing: <msg>` (`panic.c:640`). Then it stops the other CPUs, and either reboots or prints `---[ end Kernel panic - not syncing: <msg> ]---` (`panic.c:757`) and spins.
9. **Power off.** `psci_sys_poweroff` calls PSCI function `PSCI_0_2_FN_SYSTEM_OFF` (`0x8400_0008`). It uses `hvc` or `smc`, depending on the DT `/psci` `method` property, which `get_set_conduit_method` reads at `psci.c:287`.

## Key data structures

- **The Image header** (`booting.rst:79-90`): 64 bytes. `code0`, `code1`, `text_offset`, `image_size`, `flags`, three reserved words, `magic` = `"ARM\x64"`, and `res5` = the offset of the PE header.
  - Linux sets **`text_offset = 0`** (`head.S:60`).
  - `image_size` = `_end - _text`, computed in the linker script through `DEFINE_IMAGE_LE64` (`image.h:64`), so the size always includes BSS.
- **`struct fdt_header`** (`scripts/dtc/libfdt/fdt.h:12-29`): ten big-endian `u32` fields. Our `klib::fdt::FdtHeader` is the same.

## Engineering techniques worth stealing

- **A header that two loaders understand.** The first instruction is valid both as ARM code and as the "MZ" signature of a PE file. One file boots both through a bootloader and through UEFI.
- **The linker computes the header.** `image_size` is a link-time symbol, so the header can never get out of date as the kernel grows.
- **Keep boot arguments in callee-saved registers** (`x19`–`x21`) until memory and a stack are ready. Nothing in the early boot code can clobber them.
- **Ask the firmware's description, don't assume.** The PSCI conduit and the UART address both come from the device tree.

## Our Rust design and why it differs

| Linux | Ours (M1) | Why |
|---|---|---|
| `text_offset = 0` | `text_offset = 0x80000`, linked at `0x4008_0000` | QEMU puts its little boot stub at the bottom of RAM. With offset 0, QEMU quietly adds 2 MB, and the kernel would run somewhere other than where it was linked. 0x80000 gives a fixed, known address. Linux doesn't care because it relocates itself. We don't relocate yet. |
| `image_size` from `DEFINE_IMAGE_LE64` | `__image_size = __image_end - __image_start` in `kernel.ld`, emitted as `.quad __image_size` | Same idea. The assembler can't subtract two undefined symbols, so the linker does it. |
| Header word 0 = EFI "MZ" nop | `b primary_entry` | We don't support UEFI booting. |
| FDT pointer kept in `x21` | kept in `x19` until `__rust_boot` | Same technique: a callee-saved register. |
| BSS cleared by C (`map_kernel.c`) after a stack exists | cleared in assembly before Rust runs | Rust may read statics immediately, so they must be zero first. Our stack is outside BSS (`.stack` section), so clearing BSS doesn't erase it. |
| MMU on before `start_kernel` | MMU off throughout M1; it comes on in M2 | Simpler first step. The cost: all data memory counts as Device memory, so **no atomics and no unaligned access** in M1. |
| `init_kernel_el` drops from EL2 | we require EL1, and panic with a clear message at EL2 | Dropping from EL2 is on the "later" list. QEMU without `virtualization=on` starts at EL1. |
| PSCI conduit read from the DT | `hvc` at EL1, `smc` at EL2 | Temporary until the M2 device-tree walker reads `/psci` `method`. |
| UART base from the DT / earlycon | hard-coded `0x0900_0000` (QEMU `virt`) | Temporary until M2. |
| `vpanic` prints the banner, stops CPUs, spins | prints the same two banner lines plus the location, then powers off | In QEMU, powering off ends the test run immediately instead of hanging. |
| C everywhere | All `unsafe` code is in `foundation`. `start_kernel` and `panic` are in `kernel`, which is `#![forbid(unsafe_code)]` | The compiler guarantees that `kernel` contains no `unsafe` code. |
| `-mgeneral-regs-only` for C | target `aarch64-unknown-none-softfloat` | Same reason: the kernel never touches FP/SIMD registers, so the user's FP state never needs saving on kernel entry. Linux's own Rust build uses this exact target (`arch/arm64/Makefile:52`). |

**How the crates connect at boot:**

```
QEMU → _start (head.rs, global_asm)                     foundation, unsafe
     → __rust_boot(dtb, runtime, linked)  builds BootInfo  foundation, unsafe
     → __kernel_main(info)  generated by foundation::entry!   vmlinux
     → kernel::init::main::start_kernel(info)                kernel, no unsafe
```

## Open questions

- How does Linux relocate itself (`__pi_rela`, `__pi_relr`, KASLR)? We will need that if we ever load at an unknown address.
- `record_mmu_state` handles a bootloader that leaves the MMU on. QEMU never does that, but real firmware might.
