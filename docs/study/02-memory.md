# 02 — How Linux manages memory (ARM64, from the device tree to `kmalloc`)

**Linux reference:** v7.3 (`c03c1a14507d`), in `reference/linux`

**Scope:** the M2 path: "the device tree says where RAM is" → boot allocator → page tables and MMU on → page allocator → `kmalloc` → spinlocks. Page faults, `mmap`, reclaim and swap come later (M4) or never.

## Files read

- `drivers/of/fdt.c:1001-1024` (`early_init_dt_scan_root`), `:1037-1089` (`early_init_dt_scan_memory`), `:1159-1199` (`early_init_dt_add_memory_arch`), `:1206-1257` (verify and scan), `:495-532` (reserved memory and the DTB itself).
- `arch/arm64/kernel/setup.c:170-207` (`setup_machine_fdt`) and `:281-344` (`setup_arch`).
- `arch/arm64/mm/init.c:196-301` (`arm64_memblock_init`) and `:303-333` (`bootmem_init`).
- `include/linux/memblock.h:56-111` and `mm/memblock.c`: overview `:44-104`, data `:32-139`, search `:270-291`, merge/insert/add `:535-748`, isolate `:812-870`, allocate `:1559-1600`, hand-off to the buddy allocator `:2329-2503`.
- `tools/testing/memblock/README`: Linux's user-space test harness for memblock.
- `arch/arm64/include/asm/pgtable-hwdef.h:20-60, 137-195` (levels and descriptor bits), `pgtable-types.h:31` (`pte_t`).
- `arch/arm64/include/asm/memory.h:44-45, 171-175, 336-392` (VA layout, memory types, `__pa`/`__va`), `arch/arm64/mm/physaddr.c:10-28`.
- `arch/arm64/mm/proc.S:74-79, 490-570` (`__cpu_setup`), `arch/arm64/include/asm/sysreg.h:873-881` (MAIR values), `arch/arm64/kernel/head.S:459-473` (`__enable_mmu`), `arch/arm64/include/asm/assembler.h:739-754` (`set_sctlr`).
- `arch/arm64/kernel/pi/map_range.c:29-89` (early page tables, built before the MMU is on).
- `arch/arm64/mm/mmu.c:107-195` (`early_pgtable_alloc`, `pgattr_change_is_safe`, `init_pte`), `:253-290` (`init_pmd`), `:481-528` (`__create_pgd_mapping`), `:1180-1215` (`map_mem`), `:1474-1482` (`paging_init`).
- `include/linux/mm_types.h:50-223` (`struct page`).
- `include/linux/mmzone.h:31-39, 195-198, 979-1113` (orders, `free_area`, `zone`), `arch/arm64/Kconfig:1780-1784`.
- `mm/page_alloc.h:130-210` (buddy rules), `mm/page_alloc.c:916-1023` (`__free_one_page`), `:1714-1751` (`expand`), `:1900-1924` (`__rmqueue_smallest`), `:1554-1567` and `:3221-3234` (zone lock), `:616-642` (`bad_page`).
- `mm/mm_init.c:2643-2672` (`mm_core_init`).
- `include/linux/slab.h:798-820` (`kmalloc` size classes), `mm/slub.c:1-66, 531-555` (design, lock order, freelist hardening).
- `include/linux/spinlock_api_smp.h:148-157`, `kernel/locking/spinlock.c:179-183`, `arch/arm64/include/asm/spinlock.h:8`, `include/asm-generic/ticket_spinlock.h:33-69`.
- Linux's own Rust code: `rust/kernel/sync/lock.rs:37-301`, `rust/kernel/sync/locked_by.rs:9-18`, `rust/kernel/alloc.rs:23-200`, `rust/kernel/alloc/kbox.rs:104-116, 254`.

## How Linux does it

```
DTB "memory" node ──► memblock: two sorted lists of ranges ("memory", "reserved")
                         │  (page-table pages for the linear map come from here)
                         ▼
                    paging_init: map all RAM at PAGE_OFFSET
                         │
                    memblock_free_all: every free range goes to the buddy allocator
                         ▼
                    buddy allocator: per zone, one free list per order (1, 2, 4 … 1024 pages)
                         │  (slabs are pages from here)
                         ▼
                    SLUB: kmalloc size classes (8, 16, 32 … 4096 bytes)
```

### Step 1: find RAM in the device tree

1. `setup_arch` (`setup.c:281`) calls `setup_machine_fdt(__fdt_pointer)` (`setup.c:292`). It maps the DTB and reserves it: `memblock_reserve(dt_phys, size)` (`setup.c:177`).
2. `early_init_dt_verify` (`fdt.c:1206`) checks the header (`fdt_check_header`, which our M1 `klib::fdt` mirrors). Then `early_init_dt_scan_root` (`fdt.c:1001`) reads the root `#address-cells` and `#size-cells`, which say how many 32-bit cells make up one address and one size in every `reg` property.
3. `early_init_dt_scan_memory` (`fdt.c:1037`) visits every node whose `device_type` is `"memory"` (`fdt.c:1050`). It reads `linux,usable-memory`, or `reg` if that is missing (`fdt.c:1056`), and skips entries with size 0 (`fdt.c:1072`).
4. `early_init_dt_add_memory_arch` (`fdt.c:1159`) rounds the start up and the size down to whole pages (`fdt.c:1169`), clips anything above the largest physical address (`fdt.c:1181`), then calls `memblock_add` (`fdt.c:1198`).

**What QEMU passes us.** Dumped with `qemu-system-aarch64 -machine virt,dumpdtb=… -cpu cortex-a72 -m 512M`, then decoded with `dtc`:

| Node | Property | Value | Replaces this M1 shortcut |
|---|---|---|---|
| `/` | `#address-cells`, `#size-cells` | 2, 2 (each `reg` entry = 64-bit address + 64-bit size) | — |
| `/memory@40000000` | `reg` | `<0x00 0x40000000 0x00 0x20000000>` = 512 MiB at `0x4000_0000` | — |
| `/pl011@9000000` | `reg` | `<0x00 0x9000000 0x00 0x1000>` | hard-coded UART base |
| `/chosen` | `stdout-path` | `"/pl011@9000000"` | hard-coded UART base |
| `/psci` | `method` | `"hvc"`; `"smc"` with `virtualization=on` | conduit chosen by exception level |
| header | `/memreserve/` table | empty (only the `(0, 0)` end marker at offset `0x30`) | — |

The boot log will show what QEMU really passes with `-kernel`. The dump above was taken without it.

### Step 2: memblock, the boot allocator

- **What it is.** Two sorted arrays of `[base, base+size)` ranges: `memory` (what RAM exists) and `reserved` (what is taken). Free memory = `memory` minus `reserved`. See `memblock.h:75-111` and the overview at `memblock.c:44-104`.
- **No allocator needed to start.** The arrays are static, with 128 entries each (`memblock.c:32`, `:123-124`).
- **Adding a range** (`memblock_add_range`, `memblock.c:610`) has two passes:
  - The first pass counts how many new entries the range needs. If the array might overflow, it doubles the array (`memblock_double_array`, `memblock.c:706`, which allocates the new array *from memblock itself*) and starts again (`repeat:`, `memblock.c:645`).
  - The second pass inserts the pieces. Then neighbours that touch and have the same flags are merged (`memblock_merge_regions`, `memblock.c:535`).
  - The worst case, explained at `memblock.c:635-641`: a new range that overlaps every existing one needs `cnt + 1` new entries.
- **Overflow and overlap.** `memblock_cap_size` (`memblock.c:190`) shrinks `size` so `base + size` can't wrap. Two `BUG_ON`s stop the kernel if regions overlap after a merge (`memblock.c:551`) or the array is full (`memblock.c:584`).
- **Splitting.** `memblock_isolate_range` (`memblock.c:812`) splits regions at the edges of a range, so remove and mark operations always work on whole regions.
- **Allocating** is top-down by default (`.bottom_up = false`, `memblock.c:138`).
  - `__memblock_find_range_top_down` (`memblock.c:270`) walks the free ranges from the top and **returns 0 when nothing fits** (`memblock.c:290`).
  - `memblock_alloc_range_nid` (`memblock.c:1559`) then reserves what it found (`memblock.c:1587`).
  - If it is called after the slab allocator is up, it warns and calls `kzalloc` instead (`memblock.c:1572`). This is a runtime check for "wrong boot phase".
- **ARM64 additions** (`arm64_memblock_init`, `init.c:196`):
  - remove memory above the supported physical address size (`init.c:215`);
  - choose `memstart_addr`, the physical base of the linear map (`init.c:220`);
  - reserve the kernel image (`init.c:293`);
  - reserve the DT's `/memreserve/` entries and `/reserved-memory` nodes (`init.c:300` → `fdt.c:495`, loop at `fdt.c:509`).
- **Linux tests memblock outside the kernel.** `tools/testing/memblock` is a user-space "memblock simulator". It compiles the real `memblock.c` with stubbed kernel headers; the README says most stubs "don't match the kernel implementation". It is 8,652 lines in all (`git ls-files tools/testing/memblock | xargs cat | wc -l`).

### Step 3: page tables and the MMU

- **On ARM64 Linux, the MMU is already on before any C code runs.**
  - `map_range` (`pi/map_range.c:29`) builds the early tables.
  - `__cpu_setup` (`proc.S:490`) sets MAIR and TCR (`proc.S:507-547`).
  - `__enable_mmu` (`head.S:459`) loads `TTBR0`/`TTBR1` and writes `SCTLR_EL1` through `set_sctlr`, which does `msr; isb; ic iallu; dsb nsh; isb` (`assembler.h:739-750`).
- **MAIR** holds 8 one-byte memory types, and each page-table entry picks one by index (`PTE_ATTRINDX`, `pgtable-hwdef.h:195`). Linux's indices (`memory.h:171-175`, `proc.S:74-79`) and values (`sysreg.h:873-877`):

| Index | Name | MAIR byte | Used for |
|---|---|---|---|
| 0 | `MT_NORMAL` | `0xff` (write-back cacheable) | RAM |
| 3 | `MT_DEVICE_nGnRnE` | `0x00` | strictly ordered devices |
| 4 | `MT_DEVICE_nGnRE` | `0x04` | normal MMIO such as the UART |

- **Descriptor bits** (`pgtable-hwdef.h:137-176`):

| Bits | Name | Meaning |
|---|---|---|
| 0 | `PTE_VALID` | the entry is used |
| 1:0 = `3` / `1` | `PMD_TYPE_TABLE` / `PMD_TYPE_SECT` | levels 0–2: points to the next table / is a block (2 MiB at level 2) |
| 1:0 = `3` | `PTE_TYPE_PAGE` | level 3: a 4 KiB page |
| 4:2 | `AttrIndx` | which MAIR entry |
| 6 | `PTE_USER` (AP[1]) | EL0 may access |
| 7 | `PTE_RDONLY` (AP[2]) | read-only |
| 9:8 | `PTE_SHARED` | inner shareable (needed for SMP) |
| 10 | `PTE_AF` | access flag. Without it, the first access faults. |
| 11 | `PTE_NG` | not global: tagged by ASID (user mappings) |
| 53 / 54 | `PTE_PXN` / `PTE_UXN` | never execute at EL1 / EL0 |

- **Levels.** With 4 KiB pages, every table has 512 entries (`PTRS_PER_PTE`, `pgtable-hwdef.h:49`), and each level resolves 9 bits of the address (`pgtable-hwdef.h:31` and `:47`). A 48-bit address needs 4 levels: an entry maps 512 GiB at level 0, 1 GiB at level 1, 2 MiB at level 2 and 4 KiB at level 3.
  - Linux's default is 52-bit VAs (`arch/arm64/Kconfig:1532`), but it falls back to 48 bits on CPUs without that feature. QEMU's `cortex-a72` is one of them.
- **Two halves.** `TTBR0` translates the low half (user space), `TTBR1` the high half (kernel).
  - The **linear map** of all RAM starts at `PAGE_OFFSET = -(1 << VA_BITS)` (`memory.h:44-45`), which is `0xffff_0000_0000_0000` for 48 bits.
  - `__va(pa)` = `(pa - PHYS_OFFSET) | PAGE_OFFSET` (`memory.h:358`, `:392`).
  - `__pa` works only on linear-map addresses. Kernel-image symbols need `__pa_symbol` instead (`memory.h:389-390`).
  - Mixing them up is caught only at runtime, and only with `CONFIG_DEBUG_VIRTUAL`: "virt_to_phys used for non-linear address" (`physaddr.c:10-17`).
- **Mapping all RAM.** `paging_init` (`mmu.c:1474`) → `map_mem` (`mmu.c:1180`) maps every memblock range into the linear map, never executable (`NO_EXEC_MAPPINGS`, `mmu.c:1188`). Page-table pages come from memblock (`early_pgtable_alloc`, `mmu.c:107`); if none are left, it panics (`mmu.c:114`).
- **Walking the levels.**
  - `__create_pgd_mapping` (`mmu.c:514`) runs under a mutex (`mmu.c:522`) and refuses a VA and PA with different offsets inside a page (`mmu.c:495`).
  - `init_pmd` uses a 2 MiB block when the start, end and PA are all 2 MiB aligned ("try section mapping first", `mmu.c:264`). Otherwise it builds a level-3 table, which `init_pte` fills (`mmu.c:162`).
- **Break-before-make.** ARM forbids changing a live, valid entry into a different valid entry (another PA, another memory type) without first invalidating it and flushing the TLB.
  - `pgattr_change_is_safe` (`mmu.c:119`) is a pure function that answers "is this change allowed?".
  - `init_pte` calls `BUG_ON` if the answer is no (`mmu.c:178`).
- **Barriers are batched.** `init_pte` writes entries with `__set_pte_nosync` and issues the barrier once, at the end of `alloc_init_cont_pte` ("Required barriers … are deferred", `mmu.c:169-170`). Forgetting that barrier is a silent bug.

### Step 4: the buddy page allocator

- **`struct page`** (`mm_types.h:80`) is one small struct per 4 KiB frame. Together they form the "memmap".
  - Most of it is a five-word union whose meaning depends on what the page is used for (`mm_types.h:84-88`). Bit 0 of the first word must stay clear for every user (`mm_types.h:52`, `:85`).
  - A free page links into its free list through `buddy_list` (`mm_types.h:100`).
  - Its order is kept in `page->private` (`mm_types.h:116`, `page_alloc.h:136`).
  - The `PageBuddy` type marks it free (`page_alloc.h:134`), and only `zone->lock` may change that.
- **`struct zone`** (`mmzone.h:979`) has `free_area[NR_PAGE_ORDERS]` (`mmzone.h:1099`): one free list per order from 0 to `MAX_PAGE_ORDER` = 10 (`mmzone.h:31`, ARM64 4K default at `arch/arm64/Kconfig:1780-1784`). Order 10 = 1024 pages = 4 MiB.
  - Each order has a list per "migrate type", which Linux uses against fragmentation (`mmzone.h:195-198`).
  - One spinlock guards it all (`mmzone.h:1113`).
- **The buddy math** (`page_alloc.h:160-179`): the buddy of block `pfn` at order `o` is `pfn ^ (1 << o)`, and the merged parent is `pfn & ~(1 << o)`.
- **Free** (`__free_one_page`, `page_alloc.c:940`). While `order < MAX_PAGE_ORDER` (`page_alloc.c:960`), it looks up the buddy. If the buddy is free and has the same order, it takes the buddy off its list, merges (`combined_pfn = buddy_pfn & pfn`, `page_alloc.c:1005`) and moves up one order. Then it puts the result on a free list. The original explanation, signed "-- nyc", is at `page_alloc.c:916-937`.
- **Allocate** (`__rmqueue_smallest`, `page_alloc.c:1901`). It checks `free_area[order]`, then each bigger order, and takes the first block it finds. `expand` (`page_alloc.c:1714`) splits that block: it keeps the lower half each time and puts the upper halves back on the smaller lists.
- **Locking.** `zone->lock` is taken with interrupts off (`spin_lock_irqsave`, `page_alloc.c:1567`, `:3234`). Per-CPU lists (`struct per_cpu_pages`, `mmzone.h:844`) cache single pages, so most allocations never touch `zone->lock`.
- **Hand-off from memblock.**
  - `mm_core_init` (`mm_init.c:2643`) calls `memblock_free_all` (`mm_init.c:2670` → `memblock.c:2494`).
  - That loops over the free ranges (`free_low_memory_core_early`, `memblock.c:2446`) and calls `__free_pages_memory` (`memblock.c:2329`).
  - `__free_pages_memory` frees each range in the biggest blocks its alignment allows: `order = min(MAX_PAGE_ORDER, lowest set bit of start)` (`memblock.c:2342`), made smaller until the block fits (`memblock.c:2346`).
- **Catching misuse.** Freeing a page that is in a bad state prints "BUG: Bad page state" (`page_alloc.c:642`). The kernel finds out only when the bug happens.
- **GFP flags** tell the allocator what it may do. `GFP_KERNEL` may sleep and reclaim (`gfp_types.h:371`); `GFP_ATOMIC` may not (`gfp_types.h:370`).

### Step 5: SLUB and `kmalloc`

- `kmalloc` rounds a size up to a class: 8, 16, 32, 64, 96, 128, 192, 256, 512, 1024, 2048 or 4096 bytes (`slab.h:804-820`).
- A slab is one or more pages cut into equal objects. **The free list is stored inside the free objects themselves.**
  - With `CONFIG_SLAB_FREELIST_HARDENED`, each stored pointer is XORed with a per-cache random value and with its own (byte-swapped) address (`slub.c:537`). A buffer overflow that overwrites it then can't easily aim it at memory of the attacker's choice.
- The fast path uses per-CPU "sheaves" (arrays of ready objects) under a local trylock. The lock order is documented at `slub.c:59-66`.
- `mm_core_init` starts SLUB (`kmem_cache_init`, `mm_init.c:2672`) right after the buddy allocator.

### Step 6: spinlocks

- `spin_lock_irqsave` (`spinlock_api_smp.h:148`) does four things: save and disable interrupts, disable preemption, tell lockdep, take the lock. **The caller must keep the returned `flags` and hand them back to the unlock call.**
- ARM64 uses the queued spinlock (`spinlock.h:8`). The simpler generic **ticket lock** (`ticket_spinlock.h:33-69`) works in three steps:
  1. Take a ticket with one atomic `fetch_add(1 << 16)`.
  2. Spin until "now serving" equals your ticket.
  3. Unlock with a store-release of "now serving + 1".
- Which lock protects which data is a naming convention. Lockdep checks it at runtime, for example `lockdep_assert_held(&zone->lock)` (`page_alloc.c:779`).

## Key data structures

| Structure | What it holds | Who owns and frees it |
|---|---|---|
| `struct memblock` / `memblock_type` / `memblock_region` (`memblock.h:75-111`) | sorted `[base, size)` arrays for `memory` and `reserved` | static `__initdata`; thrown away after boot unless `CONFIG_ARCH_KEEP_MEMBLOCK` |
| `pte_t`, `pmd_t`, … (`pgtable-types.h:31`) | one 64-bit descriptor, wrapped in a one-field struct so the compiler can tell the levels apart | page-table pages: from memblock at boot, from the buddy allocator later |
| `struct page` (`mm_types.h:80`) | flags, a five-word union, refcount; one per frame | the memmap, allocated at boot; never freed |
| `struct zone` / `free_area` (`mmzone.h:979`, `:195`) | free lists per order and migrate type, plus counters | static per node; `zone->lock` |
| `struct slab` / `struct kmem_cache` (`mm/slab.h:116`, `:240`) | objects of one size, and the free-object list | pages from the buddy allocator, returned when empty |
| `spinlock_t` | lock word only; the data it protects is elsewhere | the structure that embeds it |

## Engineering techniques worth stealing

- **A simple boot allocator, then a hand-off.** memblock only needs two static arrays. It hands every free range to the real allocator and is then thrown away (`memblock.c:2494`).
- **Buddy math with XOR** (`page_alloc.h:165-179`). Finding a block's buddy costs one instruction.
- **Free in the largest aligned blocks** (`memblock.c:2342-2349`). A free, 4 MiB-aligned 512 MiB range becomes 128 order-10 blocks, not 131,072 single pages.
- **Turn a hardware rule into a pure function.** `pgattr_change_is_safe` (`mmu.c:119`) holds the break-before-make rule in one place that can be tested.
- **Batch barriers.** Write many entries, then issue one barrier (`mmu.c:169`).
- **Check at build time.** `BUILD_BUG_ON` in `map_mem` (`mmu.c:1201`), and `static_assert(sizeof(struct ptdesc) <= sizeof(struct page))` (`mm_types.h:627`).
- **Wrap raw numbers in one-field structs** (`pte_t`, `pgtable-types.h:31`), so a level-2 entry can't be passed where a level-3 entry is expected.
- **Harden the free list** (`slub.c:537`) and **write down the lock order** (`slub.c:59-66`).
- **Test the early allocator in user space** (`tools/testing/memblock`).

## How Rust can do better

Each item has the same three parts: the Linux line that shows the problem → the Rust technique → exactly what it rules out, and what it doesn't.

1. **Physical and virtual addresses get different types.**
   - **Linux:** `phys_addr_t` and `unsigned long` are both plain integers. Passing a kernel-image address to `__pa` is caught only by a runtime WARN, and only with `CONFIG_DEBUG_VIRTUAL` (`physaddr.c:12-13`).
   - **Rust:** zero-cost newtypes `PhysAddr`, `VirtAddr` and `Pfn`. The only way from one to the other is a function on the linear map.
   - **Rules out:** handing a virtual address to something that expects a physical one. This is a compile error, in every build.
   - **Doesn't rule out:** a wrong `PhysAddr` value computed inside `foundation`.

2. **"Not found" is not a number.**
   - **Linux:** memblock's search returns 0 when nothing fits (`memblock.c:290`), but 0 is a real RAM address on some boards. `__rmqueue_smallest` returns `NULL`.
   - **Rust:** `Option<PhysAddr>` and `Option<PageBlock>`.
   - **Rules out:** using a failed result as if it were an address. The compiler makes the caller handle `None`.

3. **Boot phases are values the compiler tracks.**
   - **Linux:** calling memblock after the slab allocator is up is caught by `WARN_ON_ONCE(slab_is_available())` at runtime (`memblock.c:1572`).
   - **Rust:** `Memblock::into_page_allocator(self)` *consumes* memblock, so after the hand-off no code can still hold it.
   - **Rules out:** boot-only allocators being used too late.
   - **Doesn't rule out:** calling things in the wrong order before the hand-off.

4. **A free page's fields exist only while it is free.**
   - **Linux:** `struct page` is a union whose meaning depends on a flag (`mm_types.h:84-88`). The order lives in `private` only while `PageBuddy` is set (`page_alloc.h:134-136`).
   - **Rust:** an `enum FrameState { Reserved, Free { order, prev, next }, Allocated { order }, … }`. The compiler won't let code read `order` out of a page that isn't `Free`.
   - **Cost:** a tag byte. A `const` assertion keeps the size small, as Linux does with `static_assert`.

5. **Free lists linked by frame number, not by pointer.**
   - **Linux:** `struct list_head buddy_list` (`mm_types.h:100`): raw pointers inside every page.
   - **Rust:** `prev` and `next` are frame numbers into one `&mut [Frame]` slice. Each is an `Option<NonZeroU32>` holding the number + 1, so "no link" costs no extra space. The whole buddy allocator is then safe code in `klib`, tested on the Mac.
   - **Size:** a 32-bit frame number covers 16 TiB of RAM. At about 12–16 bytes per frame, the bookkeeping for our 512 MiB (131,072 frames) is 1.5–2 MiB.
   - **Cost:** one index calculation per step.

6. **An allocated block is owned, and freeing gives it back.**
   - **Linux:** `__free_pages(page, order)` trusts the caller to pass the right order and to free only once. Mistakes are found later, as "BUG: Bad page state" (`page_alloc.c:642`).
   - **Rust:** `alloc(order) -> Option<PageBlock>`. `PageBlock` remembers its order and can't be copied, and `free(block)` takes it by value.
   - **Rules out:** double free and wrong-order free in safe code.
   - **Doesn't rule out:** leaks, because Rust treats leaking (`mem::forget`) as safe. We still keep a cheap state check in `free`, in case `foundation` hands out a bad block.

7. **A lock owns the data it protects.**
   - **Linux:** "`zone->lock` protects `free_area`" is a convention, checked at runtime by lockdep (`page_alloc.c:779`). The caller of `spin_lock_irqsave` must carry `flags` to the unlock.
   - **Rust:** `SpinLock<T>`. The only way to reach the `T` is through the guard that `lock()` returns, and dropping the guard unlocks and restores interrupts. Linux's own Rust code does exactly this: `Lock<T, B>` and `Guard` with `Drop` (`rust/kernel/sync/lock.rs:106`, `:201`, `:299`). It adds `LockedBy` for data guarded by a lock that lives elsewhere (`locked_by.rs:9-18`).
   - **Rules out:** touching the data without the lock, forgetting to unlock, and losing the interrupt state.
   - **Doesn't rule out:** deadlock from taking two locks in the wrong order. Lockdep still sees that and our types don't.

8. **Bad page-table entries can't be written, and a forbidden change is a `Result`.**
   - **Linux:** attributes are bit masks, so writable and executable can be OR-ed together. Linux removes W+X later with policies (`NO_EXEC_MAPPINGS`, `mmu.c:1188`). A break-before-make violation is a `BUG_ON` (`mmu.c:178`).
   - **Rust:** `enum Access { ReadOnly, ReadWrite, ReadExecute }` has no read-write-execute variant, and `enum MemType { Normal, Device }`. One function turns them into descriptor bits. The `pgattr_change_is_safe` logic becomes a pure `klib` function that returns `Err`, and it is tested on the Mac.
   - **Rules out:** a single mapping that is both writable and executable.
   - **Doesn't rule out:**
     - The same code being writable through a *second* mapping. The linear map covers all RAM, kernel image included. Linux has the same problem: it maps the image's linear alias separately so it can remove the write permission later (`mmu.c:1212-1220`). See open question 6.
     - Wrong bits in that one encoding function, which tests must catch.

9. **Changing a live table hands back a "flush" value that must be used.**
   - **Linux:** the barrier after `__set_pte_nosync` is promised by a comment (`mmu.c:169-170`).
   - **Rust:** `map()` on a live table returns `#[must_use] TlbFlush`. Ignoring it is a warning, and our clippy run treats warnings as errors.
   - **Rules out:** forgetting the flush.
   - **Doesn't rule out:** flushing too little, for example one address instead of a whole range.

10. **Arithmetic overflow is handled on purpose.**
    - **Linux:** `base + size - 1` (`fdt.c:1181`) can wrap for a hostile DTB. `memblock_cap_size` (`memblock.c:190`) exists to stop that kind of wrap.
    - **Rust:** a `PhysRange` can only be built through `checked_add`, which gives an `Err` instead of a wrapped number.
    - **Note:** Rust only panics on overflow in debug builds; release builds wrap. So checked arithmetic must be written out on purpose, as M1's `FdtHeader::parse` already does.

11. **Host tests without a simulator.**
    - **Linux:** testing memblock outside the kernel needs an 8,652-line harness of stubbed headers that "don't match the kernel implementation".
    - **Rust:** `klib` compiles unchanged for the Mac and for the kernel. The DT walker, memblock, the buddy allocator and the page-table builder run as plain `cargo test`. The page-table builder can even be checked by a software walker: `translate(va) -> Option<(PhysAddr, attrs)>`.

12. **Running out of memory is a value, not a crash.**
    - **Linux:** `kmalloc` returns `NULL` and every caller checks.
    - **Rust:** standard Rust's `Box::new` and `Vec::push` call `handle_alloc_error` and stop the program when memory runs out. Linux's own Rust code therefore does *not* use them. It has its own `Box<T, A: Allocator>` (`kbox.rs:104`), where `KBox::new(x, GFP_KERNEL)` returns `Result<Self, AllocError>` (`kbox.rs:254`). It removed its fork of Rust's `alloc` crate in commit `9d0441bab775` (2024-03-27).
    - **For us:** see open question 2.

**Small extra:** Rust's `GlobalAlloc::dealloc(ptr, layout)` is told the size, so the heap knows the size class without looking it up. It still has to find the slab from the pointer to mark the slot free, just as Linux's `kfree(ptr)` does.

### Where Linux's way is still better

- **Speed under load.** Per-CPU page lists (`mmzone.h:844`), SLUB's per-CPU sheaves and the queued spinlock all scale better than what we will build first. We add per-CPU parts in M7.
- **Memory.** Linux keeps the free-object pointer inside the free object, which costs no extra memory. If we keep slab bookkeeping outside the objects (a free bitmap per slab), safe code never writes into memory it has handed out, and an overflowing object can't corrupt the free list. The cost is some extra memory and a bit-search per allocation.
- **The hardware is outside the type system.** TLB invalidation, cache maintenance, barriers, and the table walker reading memory behind our back are all invisible to Rust's types. That code stays `unsafe` in `foundation` and is checked by QEMU tests, as in Linux.
- **Decades of tools.** KASAN, KFENCE, page poisoning, lockdep's lock-order proofs, compaction and NUMA. We have none of them, and we don't need most of them yet.

## Our Rust design and why it differs (proposal for the M2 plan)

### The boot order is different from Linux's

Linux turns the MMU on with small static tables *before* any C code runs, so memblock and `paging_init` run with the MMU already on. Our M1 kernel runs Rust with the MMU **off**. So in M2:

1. **MMU off, M1 rules still apply** (no atomics, aligned access only):
   1. Walk the DTB (`klib`).
   2. Fill memblock. It is an ordinary owned local value, with no lock and no static, because nothing else runs yet.
   3. Reserve the kernel image (which includes our stack) and the DTB.
   4. Take page-table pages from memblock and build the tables.
2. **MMU on:**
   1. Set MAIR, TCR and the TTBRs, then set `SCTLR_EL1.{M,C,I}` with Linux's `isb` sequence.
   2. From here on, atomics are allowed, so `SpinLock` is too.
3. **Hand-offs:**
   1. `memblock.into_page_allocator()` creates the buddy allocator.
   2. The heap (slab size classes) is built on top of the buddy allocator.
   3. `Vec`, `Box` and `Arc` work.

**The switch must map, at minimum:**
- the running kernel image, which includes the stack;
- the DTB, because `BootInfo.dtb` still points at physical `0x4800_0000` (map it there, or move the pointer into the linear map);
- the UART, as Device memory;
- the page tables themselves. We build them with the MMU off and must change them later.

**One rule for the switch:** a Rust reference made with the MMU off holds a physical address. After the switch, it still works only if that address is mapped at the same place. So any memory outside the image and the DTB must be reached again through the linear map. The compiler can enforce this: `enable_mmu(early: EarlyTables) -> MmuOn` *consumes* the pre-MMU handles and gives back linear-map versions. This is "boot phases are values" (improvement 3) again.

If something is not mapped, the next access faults. We have no exception vectors until M3, so a missing mapping shows up only as a silent hang and a 10-second test timeout (see open question 4).

### Where each piece lives

| Piece | Crate / path | Linux counterpart | `unsafe`? |
|---|---|---|---|
| DTB structure walker (nodes, properties, `reg` cells) | `klib/src/fdt.rs` (extend) | `scripts/dtc/libfdt/`, `drivers/of/fdt.c` | no |
| `PhysAddr`, `VirtAddr`, `Pfn`, `PhysRange` | `klib/src/mm/addr.rs` | `phys_addr_t`, `__pa`/`__va` | no |
| memblock (fixed 128-entry arrays, `Result` when full) | `klib/src/mm/memblock.rs` | `mm/memblock.c` | no |
| buddy allocator over `&mut [Frame]` | `klib/src/mm/buddy.rs` | `mm/page_alloc.c` | no |
| slab size classes (bookkeeping outside objects) | `klib/src/mm/slab.rs` | `mm/slub.c` (ideas only) | no |
| ARM64 descriptor encoding, table builder, break-before-make check | `klib/src/arch/aarch64/pgtable.rs` (see open question 3) | `pgtable-hwdef.h`, `mmu.c`, `pi/map_range.c` | no |
| MAIR/TCR/TTBR/SCTLR, `dsb`/`isb`/`tlbi` | `foundation/src/arch/aarch64/mmu.rs` | `proc.S`, `head.S` | yes |
| turning a physical range into `&'static mut [Frame]` and page slices | `foundation/src/mm.rs` | the memmap, `__va` | yes |
| `GlobalAlloc` (pointer ↔ slab slot) | `foundation/src/mm/heap.rs` | `kmalloc`/`kfree` | yes |
| `SpinLock<T>` (ticket lock) and guard | `foundation/src/sync/spinlock.rs` | `ticket_spinlock.h`, `spinlock_api_smp.h` | yes |
| boot sequence, `Memory: …` log line, stress test | `kernel/src/mm/` and `kernel/src/init/main.rs` | `mm/mm_init.c`, `arch/arm64/mm/init.c` | no (forbidden) |

### Linux → ours

| Linux | Ours (M2) | Why |
|---|---|---|
| MMU on before C (`pi/map_range.c`) | MMU on from Rust, after memblock | Our Rust already runs MMU-off; building tables in Rust lets `klib` test them on the Mac. |
| memblock arrays double themselves from memblock | fixed 128 entries, `Err` when full | No self-allocation trick. QEMU gives us 1 memory range and a few reservations. |
| two-pass add with `goto repeat` | build the new sorted array in one pass, after checking capacity | Same result with less code, because the array never grows. |
| `struct page` union + `PageBuddy` + `private` | `enum FrameState` with fields per state | The compiler checks the state before any field is read. |
| `list_head` pointers | `Option<u32>` frame-number links | Lets the buddy allocator be safe and host-tested. |
| `__free_pages(page, order)` | `free(block: PageBlock)` by value | No wrong-order or double free in safe code. |
| `spinlock_t` next to the data + lockdep | `SpinLock<T>` with a guard | Same as Rust-for-Linux. Data can't be reached without the lock. |
| qspinlock | ticket lock | Fair, about 20 lines, and enough for `-smp 4`. Linux keeps a generic ticket lock too. |
| SLUB free pointer inside objects | free bitmap per slab, outside objects | Safe code never writes into handed-out memory. |
| `kmalloc` → `NULL` | `GlobalAlloc`, OOM = panic (for now) | The roadmap wants `Vec`/`Box`/`Arc` in M2. See open question 2. |
| zones (DMA, DMA32, NORMAL), migrate types, per-CPU lists | one zone, one free list per order | QEMU has no DMA limits and we have one CPU until M7. |

## Open questions (decisions for the M2 plan)

1. **Virtual address layout.**
   - **A.** Identity map only (VA = PA in `TTBR0`). This is simplest, but it isn't Linux, and in M4 user space will want `TTBR0` back.
   - **B (recommended).** Keep the kernel image identity-mapped in `TTBR0` for now, and put Linux's linear map of all RAM in `TTBR1` at `PAGE_OFFSET = 0xffff_0000_0000_0000`. `__va`/`__pa` then work like Linux from M2 on, and all allocator memory is reached through the linear map. Later, only the kernel image has to move high (link at a high address and add an early stage); the allocators don't change.
   - **C.** The full Linux layout now: link the kernel high and run position-independent code until the MMU is on. This is the most faithful, but it means a lot of tricky pre-MMU work in M2.
2. **Heap API.**
   - **A (recommended for M2).** `GlobalAlloc`, as the roadmap row says. `Vec`/`Box`/`Arc` work, and running out of memory panics with a clear message.
   - **B.** Fallible allocation from the start, like Rust-for-Linux (`KBox::new(x, flags) -> Result`).
   - Recommendation: A now, and decide again before M4, when user programs can make the kernel allocate.
3. **ARM64 data formats in `klib`.** The global constraint says CPU-specific code lives only in `foundation/src/arch/`. But `foundation` doesn't build for the Mac, so the page-table builder couldn't be unit-tested there. Recommendation: allow `klib/src/arch/aarch64/` for **pure data formats** (descriptor bits, table building), with no `cfg` and no hardware access. The rule's purpose, "no `#[cfg(target_arch)]` in generic code", still holds.
4. **A minimal fault reporter in M2?** Without exception vectors, every MMU mistake is a silent hang. Recommendation: yes. Add a tiny vector table that prints `ESR_EL1`, `FAR_EL1` and `ELR_EL1` and powers off, about 40 lines. M3 replaces it with real handlers.
5. **Guarding "no atomics before the MMU"** (M1 review minor #7). We could check `SCTLR_EL1.M` in `SpinLock::lock()` in debug builds, or pass an `MmuOn` token. Recommendation: the runtime check. It's one `mrs` instruction and needs no API changes.
6. **Write-protect the kernel's own code.**
   - Mapping text as `ReadExecute` and data as `ReadWrite` needs section boundaries on 4 KiB boundaries. Our linker script aligns them to 8 bytes today, so M2 must change `kernel.ld`.
   - With layout B, the linear map also contains the kernel image, and it is writable there. Linux maps that part separately so it can make it read-only later (`mmu.c:1212-1220`). Recommendation: do the same. Map the image's linear alias with its own entries, as `ReadOnly` for text, so code can't be changed through the alias.
7. **Caches on real hardware.** Tables written with the MMU and caches off may need cache invalidation before a cacheable table walk (Linux invalidates its early tables with `dcache_inval_poc`, `head.S:98-107`). QEMU's TCG mode doesn't model caches, so we note it and skip it for now.
8. **Recursive-panic guard.** It needs an atomic flag. It becomes possible in M2, once the MMU is on.
