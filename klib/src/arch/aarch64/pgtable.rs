// SPDX-License-Identifier: GPL-2.0-only
//! ARM64 page tables: MAIR/TCR values, descriptor bits, the table builder
//! and a software walk. Linux counterpart: `arch/arm64/include/asm/
//! pgtable-hwdef.h`, `pgtable-prot.h`, `arch/arm64/mm/mmu.c` and the
//! MAIR/TCR setup in `arch/arm64/mm/proc.S`.
//!
//! The format is 4 KiB pages, 48-bit virtual addresses and 4 levels
//! (0 to 3). Level 2 entries can be 2 MiB blocks, level 3 entries are
//! pages.

use crate::mm::addr::{MAX_PHYS, PAGE_SHIFT, PAGE_SIZE, PhysAddr, VirtAddr};
use crate::mm::map::{Access, Attrs, MapError, Mapping, MemType};

/// MAIR_EL1 attribute indices (`arch/arm64/include/asm/memory.h:171-175`).
pub const MT_NORMAL: u64 = 0;
pub const MT_DEVICE_NGNRE: u64 = 4;

/// Attr0 = Normal write-back (`MAIR_ATTR_NORMAL`, 0xff), Attr4 =
/// Device-nGnRE (0x04), placed as `MAIR_ATTRIDX` does
/// (`arch/arm64/include/asm/sysreg.h:874-881`).
pub const MAIR_EL1_VALUE: u64 = (0xff << (8 * MT_NORMAL)) | (0x04 << (8 * MT_DEVICE_NGNRE));

/// TCR_EL1 without IPS (`arch/arm64/mm/proc.S:508-509`, fields from
/// `arch/arm64/tools/sysreg`):
/// - T0SZ = T1SZ = 16: 48-bit virtual addresses in both halves;
/// - IRGN/ORGN = 0b01, write-back write-allocate; SH = 0b11, inner shareable;
/// - TG0 = 0b00 is 4 KiB, but TG1 = 0b10 is 4 KiB (the encodings differ).
const TCR_BASE: u64 = {
    // TG0 (bits 15:14) = 0b00 adds no bits.
    let t0 = 16 | (0b01 << 8) | (0b01 << 10) | (0b11 << 12);
    let t1 = (16 << 16) | (0b01 << 24) | (0b01 << 26) | (0b11 << 28) | (0b10 << 30);
    t0 | t1
};

/// PARange/IPS value for 48 bits. Without LPA2, Linux narrows a larger
/// PARange to this (`tcr_compute_pa_size`, `assembler.h:342-355`).
const PARANGE_48: u8 = 0b101;

/// The TCR_EL1 value for a CPU whose `ID_AA64MMFR0_EL1.PARange` is
/// `pa_range` (bits 3:0). IPS is capped at 48 bits.
pub const fn tcr_el1(pa_range: u8) -> u64 {
    let mut ips = pa_range & 7;
    if ips > PARANGE_48 {
        ips = PARANGE_48;
    }
    TCR_BASE | ((ips as u64) << 32)
}

/// Entries per table: one 4 KiB page of 8-byte descriptors.
pub const ENTRIES: usize = 512;
pub type Table = [u64; ENTRIES];

// Descriptor bits (`arch/arm64/include/asm/pgtable-hwdef.h:137-196`).
const PTE_VALID: u64 = 1 << 0;
const PTE_TYPE_MASK: u64 = 0b11;
/// A table entry (levels 0-2) and a page entry (level 3) look the same.
const PTE_TYPE_TABLE_OR_PAGE: u64 = 0b11;
const PMD_TYPE_SECT: u64 = 0b01;
const PTE_RDONLY: u64 = 1 << 7;
const PTE_SHARED: u64 = 3 << 8;
const PTE_AF: u64 = 1 << 10;
const PTE_NG: u64 = 1 << 11;
/// `PTE_WRITE` is `PTE_DBM` (`pgtable-prot.h:16`). We never set it, but
/// `change_is_safe` ignores it as Linux does.
const PTE_DBM: u64 = 1 << 51;
const PTE_PXN: u64 = 1 << 53;
const PTE_UXN: u64 = 1 << 54;
const PTE_SWBITS_MASK: u64 = (1 << 63) | (0xf << 55);
/// Output address bits 47:12.
const PTE_ADDR_MASK: u64 = (MAX_PHYS - 1) & !(PAGE_SIZE - 1);

const fn pte_attrindx(t: u64) -> u64 {
    t << 2
}

/// log2 of the size one entry covers at `level`: 39, 30, 21, 12.
const fn level_shift(level: u8) -> u32 {
    PAGE_SHIFT + 9 * (3 - level as u32)
}

fn index(va: VirtAddr, level: u8) -> usize {
    ((va.as_u64() >> level_shift(level)) as usize) & (ENTRIES - 1)
}

const BLOCK_LEVEL: u8 = 2;
const PAGE_LEVEL: u8 = 3;
const BLOCK_SIZE: u64 = 1 << level_shift(BLOCK_LEVEL);

/// A level-2 block (2 MiB) or level-3 page descriptor for `pa`, built from
/// `PROT_NORMAL`/`PROT_DEVICE_nGnRE` (`pgtable-prot.h:38-50`): AF, inner
/// shareable, UXN always, PXN unless `ReadExecute`, RDONLY unless
/// `ReadWrite`. No nG and no software bits. Any other level is
/// `Misaligned`, since we place no block of that size.
pub fn leaf_descriptor(pa: PhysAddr, attrs: Attrs, level: u8) -> Result<u64, MapError> {
    let (size, kind) = match level {
        BLOCK_LEVEL => (BLOCK_SIZE, PMD_TYPE_SECT),
        PAGE_LEVEL => (PAGE_SIZE, PTE_TYPE_TABLE_OR_PAGE),
        _ => return Err(MapError::Misaligned),
    };
    if attrs.mem == MemType::Device && attrs.access == Access::ReadExecute {
        return Err(MapError::ExecutableDevice);
    }
    if !pa.is_aligned(size) {
        return Err(MapError::Misaligned);
    }
    if pa.as_u64() >= MAX_PHYS {
        return Err(MapError::OutOfRange);
    }
    let attrindx = match attrs.mem {
        MemType::Normal => pte_attrindx(MT_NORMAL),
        MemType::Device => pte_attrindx(MT_DEVICE_NGNRE),
    };
    let access = match attrs.access {
        Access::ReadWrite => PTE_PXN,
        Access::ReadOnly => PTE_PXN | PTE_RDONLY,
        Access::ReadExecute => PTE_RDONLY,
    };
    Ok(pa.as_u64() | kind | attrindx | PTE_SHARED | PTE_AF | PTE_UXN | access)
}

/// A table descriptor pointing at the next-level table at `next`.
pub const fn table_descriptor(next: PhysAddr) -> u64 {
    next.as_u64() | PTE_TYPE_TABLE_OR_PAGE
}

/// Whether a live entry may change from `old` to `new` without
/// break-before-make: `pgattr_change_is_safe` (`arch/arm64/mm/mmu.c:119-152`).
/// We have no Normal-Tagged type, so its Normal <-> Tagged rule is left out.
pub fn change_is_safe(old: u64, new: u64) -> bool {
    let mask = PTE_PXN | PTE_RDONLY | PTE_DBM | PTE_NG | PTE_SWBITS_MASK;
    // Creating or taking down a mapping is always safe.
    if old & PTE_VALID == 0 || new & PTE_VALID == 0 {
        return true;
    }
    // A live entry's pfn must not change.
    if old & PTE_ADDR_MASK != new & PTE_ADDR_MASK {
        return false;
    }
    // Non-global to global is unsafe.
    if old & !new & PTE_NG != 0 {
        return false;
    }
    (old ^ new) & !mask == 0
}

/// Where the page tables live. The builder only knows tables by their
/// physical address; the implementation turns that into memory.
pub trait TableMemory {
    /// A new, all-zero table, or `None` if there is no memory left.
    fn alloc_table(&mut self) -> Option<PhysAddr>;
    fn table(&self, pa: PhysAddr) -> &Table;
    fn table_mut(&mut self, pa: PhysAddr) -> &mut Table;
}

/// Maps `m` into the tables under `root`, with 2 MiB blocks where `va`,
/// `pa` and the remaining size allow (`mmu.c:264`) and 4 KiB pages
/// elsewhere. Linux counterpart: `__create_pgd_mapping`.
///
/// `va`, `pa` and `size` must be page-aligned (Linux rounds them; we refuse,
/// `mmu.c:495`). The virtual range must stay in its half of the address
/// space and the physical range below 2^48. A live entry is only changed
/// when [`change_is_safe`] allows it; a page inside a block or a block over
/// a table is `BreakBeforeMake`. On `OutOfTables` or `BreakBeforeMake` the
/// part before the failing address stays mapped.
pub fn map(mem: &mut impl TableMemory, root: PhysAddr, m: &Mapping) -> Result<(), MapError> {
    if !(m.va.as_u64() | m.pa.as_u64() | m.size).is_multiple_of(PAGE_SIZE) {
        return Err(MapError::Misaligned);
    }
    if m.attrs.mem == MemType::Device && m.attrs.access == Access::ReadExecute {
        return Err(MapError::ExecutableDevice);
    }
    if m.size == 0 {
        return Ok(());
    }
    let va_last = m.va.as_u64().checked_add(m.size - 1).ok_or(MapError::OutOfRange)?;
    let pa_end = m.pa.as_u64().checked_add(m.size).ok_or(MapError::OutOfRange)?;
    if va_last >> 48 != m.va.as_u64() >> 48 || pa_end > MAX_PHYS {
        return Err(MapError::OutOfRange);
    }

    let mut done = 0;
    while done < m.size {
        let (va, pa) = (m.va.as_u64() + done, m.pa.as_u64() + done);
        let block = (va | pa).is_multiple_of(BLOCK_SIZE) && m.size - done >= BLOCK_SIZE;
        let level = if block { BLOCK_LEVEL } else { PAGE_LEVEL };
        let desc = leaf_descriptor(PhysAddr::new(pa), m.attrs, level)?;
        // `va` is canonical: it is in the same half as `m.va`.
        set_leaf(mem, root, VirtAddr::new(va).ok_or(MapError::OutOfRange)?, level, desc)?;
        done += if block { BLOCK_SIZE } else { PAGE_SIZE };
    }
    Ok(())
}

/// Walks from `root` to `level` for `va`, making missing tables, and
/// writes `desc` there.
fn set_leaf(
    mem: &mut impl TableMemory,
    root: PhysAddr,
    va: VirtAddr,
    level: u8,
    desc: u64,
) -> Result<(), MapError> {
    let bbm = MapError::BreakBeforeMake { va: va.as_u64() };
    let mut table = root;
    for l in 0..level {
        let i = index(va, l);
        let entry = mem.table(table)[i];
        table = if entry & PTE_VALID == 0 {
            let next = mem.alloc_table().ok_or(MapError::OutOfTables)?;
            if !next.is_aligned(PAGE_SIZE) {
                return Err(MapError::Misaligned);
            }
            if next.as_u64() >= MAX_PHYS {
                return Err(MapError::OutOfRange);
            }
            mem.table_mut(table)[i] = table_descriptor(next);
            next
        } else if entry & PTE_TYPE_MASK == PTE_TYPE_TABLE_OR_PAGE {
            PhysAddr::new(entry & PTE_ADDR_MASK)
        } else {
            // A block already covers `va`.
            return Err(bbm);
        };
    }
    let i = index(va, level);
    // A block over a table also fails here: a table descriptor has no AF.
    if !change_is_safe(mem.table(table)[i], desc) {
        return Err(bbm);
    }
    mem.table_mut(table)[i] = desc;
    Ok(())
}

/// The physical address `va` maps to under `root`, and the leaf descriptor
/// that maps it. `None` if `va` is not mapped. Only VA bits 47:12 choose
/// the entries; the caller picks the root for the half `va` is in.
pub fn translate(mem: &impl TableMemory, root: PhysAddr, va: VirtAddr) -> Option<(PhysAddr, u64)> {
    let mut table = root;
    for level in 0..=PAGE_LEVEL {
        let entry = mem.table(table)[index(va, level)];
        let kind = entry & PTE_TYPE_MASK;
        if entry & PTE_VALID == 0 {
            return None;
        }
        let leaf = match level {
            // Levels 1 and 2 may hold blocks (1 GiB, 2 MiB); level 0 may not.
            1 | 2 if kind == PMD_TYPE_SECT => true,
            PAGE_LEVEL if kind == PTE_TYPE_TABLE_OR_PAGE => true,
            _ if kind == PTE_TYPE_TABLE_OR_PAGE => false,
            _ => return None,
        };
        if leaf {
            let offset_mask = (1u64 << level_shift(level)) - 1;
            let base = entry & PTE_ADDR_MASK & !offset_mask;
            return Some((PhysAddr::new(base | (va.as_u64() & offset_mask)), entry));
        }
        table = PhysAddr::new(entry & PTE_ADDR_MASK);
    }
    None
}

#[cfg(test)]
pub(crate) mod testing {
    use super::{ENTRIES, Table, TableMemory};
    use crate::mm::addr::{PAGE_SIZE, PhysAddr};

    /// The first fake table address.
    const BASE: u64 = 0x1000_0000;

    /// Tables in host memory, at fake physical addresses
    /// `0x1000_0000 + i * 4096`, with an optional limit on how many it
    /// hands out.
    pub(crate) struct VecTables {
        tables: Vec<Box<Table>>,
        limit: Option<usize>,
    }

    impl VecTables {
        pub(crate) fn new(limit: Option<usize>) -> Self {
            VecTables { tables: Vec::new(), limit }
        }

        pub(crate) fn tables_used(&self) -> usize {
            self.tables.len()
        }

        fn index(pa: PhysAddr) -> usize {
            ((pa.as_u64() - BASE) / PAGE_SIZE) as usize
        }
    }

    impl TableMemory for VecTables {
        fn alloc_table(&mut self) -> Option<PhysAddr> {
            if self.limit.is_some_and(|l| self.tables.len() >= l) {
                return None;
            }
            self.tables.push(Box::new([0; ENTRIES]));
            Some(PhysAddr::new(BASE + (self.tables.len() as u64 - 1) * PAGE_SIZE))
        }

        fn table(&self, pa: PhysAddr) -> &Table {
            &self.tables[Self::index(pa)]
        }

        fn table_mut(&mut self, pa: PhysAddr) -> &mut Table {
            &mut self.tables[Self::index(pa)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::VecTables;
    use super::*;
    use crate::mm::addr::{PhysAddr, VirtAddr};
    use crate::mm::layout::PAGE_OFFSET;
    use crate::mm::map::{Access, Attrs, MapError, Mapping, MemType};

    const RW: Attrs = Attrs { access: Access::ReadWrite, mem: MemType::Normal };
    const SZ_2M: u64 = 2 << 20;

    fn identity(pa: u64, size: u64, attrs: Attrs) -> Mapping {
        Mapping { va: VirtAddr::new(pa).unwrap(), pa: PhysAddr::new(pa), size, attrs }
    }

    fn va(addr: u64) -> VirtAddr {
        VirtAddr::new(addr).unwrap()
    }

    #[test]
    fn mair_matches_linux_indices() {
        assert_eq!(MAIR_EL1_VALUE, 0x0000_0004_0000_00ff);
        assert_eq!((MT_NORMAL, MT_DEVICE_NGNRE), (0, 4));
    }

    #[test]
    fn tcr_is_48_bit_4k_inner_shareable_wbwa() {
        let t = tcr_el1(0b010);
        assert_eq!(t, 0x0000_0002_b510_3510);
        assert_eq!((t >> 14) & 3, 0b00, "TG0 4K");
        assert_eq!((t >> 30) & 3, 0b10, "TG1 4K");
    }

    #[test]
    fn tcr_caps_the_physical_address_size_at_48_bits() {
        assert_eq!((tcr_el1(0b101) >> 32) & 7, 0b101);
        assert_eq!((tcr_el1(0b110) >> 32) & 7, 0b101, "52-bit PA needs LPA2");
        assert_eq!((tcr_el1(0xff) >> 32) & 7, 0b101);
        assert_eq!(tcr_el1(0b110) & !(7 << 32), 0x0000_0000_b510_3510);
    }

    #[test]
    fn kernel_page_descriptors() {
        let pa = PhysAddr::new(0x4008_0000);
        assert_eq!(leaf_descriptor(pa, RW, 3), Ok(0x0060_0000_4008_0703));
        assert_eq!(
            leaf_descriptor(pa, Attrs { access: Access::ReadExecute, ..RW }, 3),
            Ok(0x0040_0000_4008_0783)
        );
        assert_eq!(
            leaf_descriptor(pa, Attrs { access: Access::ReadOnly, ..RW }, 3),
            Ok(0x0060_0000_4008_0783)
        );
        assert_eq!(
            leaf_descriptor(PhysAddr::new(0x0900_0000), Attrs { mem: MemType::Device, ..RW }, 3),
            Ok(0x0060_0000_0900_0713)
        );
        assert_eq!(leaf_descriptor(PhysAddr::new(0x4000_0000), RW, 2), Ok(0x0060_0000_4000_0701));
    }

    #[test]
    fn table_descriptor_is_the_address_with_type_bits() {
        assert_eq!(table_descriptor(PhysAddr::new(0x1000_0000)), 0x1000_0003);
    }

    #[test]
    fn leaf_descriptor_rejects_bad_alignment_level_and_address() {
        assert_eq!(leaf_descriptor(PhysAddr::new(0x4000_1000), RW, 2), Err(MapError::Misaligned));
        assert_eq!(leaf_descriptor(PhysAddr::new(0x4000_0800), RW, 3), Err(MapError::Misaligned));
        for level in [0, 1, 4] {
            assert_eq!(leaf_descriptor(PhysAddr::new(0), RW, level), Err(MapError::Misaligned));
        }
        assert_eq!(leaf_descriptor(PhysAddr::new(1 << 48), RW, 3), Err(MapError::OutOfRange));
    }

    #[test]
    fn executable_device_memory_is_rejected() {
        let rx_dev = Attrs { access: Access::ReadExecute, mem: MemType::Device };
        assert_eq!(
            leaf_descriptor(PhysAddr::new(0x0900_0000), rx_dev, 3),
            Err(MapError::ExecutableDevice)
        );
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        assert_eq!(map(&mut t, root, &identity(0x0900_0000, 0x1000, rx_dev)), Err(MapError::ExecutableDevice));
        assert_eq!(t.tables_used(), 1, "nothing was mapped");
    }

    #[test]
    fn change_is_safe_matches_linux() {
        let pa = PhysAddr::new(0x4008_0000);
        let rw = leaf_descriptor(pa, RW, 3).unwrap();
        let ro = leaf_descriptor(pa, Attrs { access: Access::ReadOnly, ..RW }, 3).unwrap();
        let other = leaf_descriptor(PhysAddr::new(0x4008_1000), RW, 3).unwrap();
        let dev = leaf_descriptor(pa, Attrs { mem: MemType::Device, ..RW }, 3).unwrap();
        const NG: u64 = 1 << 11;
        assert!(change_is_safe(0, rw), "invalid -> valid");
        assert!(change_is_safe(rw, 0), "valid -> invalid");
        assert!(change_is_safe(rw, ro), "RW -> RO, same pfn");
        assert!(change_is_safe(ro, rw), "RO -> RW, same pfn");
        assert!(!change_is_safe(rw, other), "pfn change");
        assert!(!change_is_safe(rw, dev), "Normal -> Device");
        assert!(!change_is_safe(rw | NG, rw), "nG -> global");
        assert!(change_is_safe(rw, rw | NG), "global -> nG");
    }

    #[test]
    fn linear_map_in_ttbr1_translates_back() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        let m = Mapping { va: va(PAGE_OFFSET), pa: PhysAddr::new(0x4000_0000), size: 0x2000_0000, attrs: RW };
        map(&mut t, root, &m).unwrap();
        assert_eq!(
            translate(&t, root, va(0xffff_0000_1234_5678)).map(|x| x.0),
            Some(PhysAddr::new(0x5234_5678))
        );
        assert_eq!(t.tables_used(), 3); // root + L1 + L2, with 256 blocks
        let (_, desc) = translate(&t, root, va(PAGE_OFFSET)).unwrap();
        assert_eq!(desc, leaf_descriptor(PhysAddr::new(0x4000_0000), RW, 2).unwrap(), "a 2 MiB block");
        assert_eq!(translate(&t, root, va(PAGE_OFFSET + 0x2000_0000)), None);
    }

    #[test]
    fn unaligned_range_uses_pages() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        map(&mut t, root, &identity(0x4008_0000, 0x3000, RW)).unwrap();
        let (pa, desc) = translate(&t, root, va(0x4008_2fff)).unwrap();
        assert_eq!(pa, PhysAddr::new(0x4008_2fff));
        assert_eq!(desc, leaf_descriptor(PhysAddr::new(0x4008_2000), RW, 3).unwrap(), "a 4 KiB page");
        assert_eq!(translate(&t, root, va(0x4008_3000)), None);
        assert_eq!(translate(&t, root, va(0x4007_f000)), None);
        assert_eq!(t.tables_used(), 4); // root + L1 + L2 + L3
    }

    #[test]
    fn misaligned_mapping_is_rejected() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        let m = Mapping { va: va(0x4008_0000), pa: PhysAddr::new(0x4008_0800), size: 0x1000, attrs: RW };
        assert_eq!(map(&mut t, root, &m), Err(MapError::Misaligned));
        let m = Mapping { va: va(0x4008_0800), pa: PhysAddr::new(0x4008_0000), ..m };
        assert_eq!(map(&mut t, root, &m), Err(MapError::Misaligned));
        assert_eq!(map(&mut t, root, &identity(0x4008_0000, 0x800, RW)), Err(MapError::Misaligned));
        assert_eq!(t.tables_used(), 1, "nothing was mapped");
    }

    #[test]
    fn remapping_same_entry_is_ok_but_new_pa_is_break_before_make() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        let m = identity(0x4008_0000, 0x1000, RW);
        map(&mut t, root, &m).unwrap();
        assert_eq!(map(&mut t, root, &m), Ok(()));
        let ro = Mapping { attrs: Attrs { access: Access::ReadOnly, ..RW }, ..m };
        assert_eq!(map(&mut t, root, &ro), Ok(()), "a permission change is safe");
        assert_eq!(translate(&t, root, va(0x4008_0000)).unwrap().1 & (1 << 7), 1 << 7, "now RDONLY");
        let moved = Mapping { pa: PhysAddr::new(0x4009_0000), ..m };
        assert_eq!(map(&mut t, root, &moved), Err(MapError::BreakBeforeMake { va: 0x4008_0000 }));
        assert_eq!(translate(&t, root, va(0x4008_0000)).map(|x| x.0), Some(PhysAddr::new(0x4008_0000)));
    }

    #[test]
    fn page_inside_existing_block_is_rejected() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        map(&mut t, root, &identity(0x4000_0000, SZ_2M, RW)).unwrap();
        assert_eq!(
            map(&mut t, root, &identity(0x4000_3000, 0x1000, RW)),
            Err(MapError::BreakBeforeMake { va: 0x4000_3000 })
        );
    }

    #[test]
    fn block_over_existing_table_is_rejected() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        map(&mut t, root, &identity(0x4000_0000, 0x1000, RW)).unwrap();
        assert_eq!(
            map(&mut t, root, &identity(0x4000_0000, SZ_2M, RW)),
            Err(MapError::BreakBeforeMake { va: 0x4000_0000 })
        );
        assert_eq!(translate(&t, root, va(0x4000_1000)), None, "the page table is still there");
    }

    #[test]
    fn running_out_of_tables_is_an_error() {
        let mut t = VecTables::new(Some(1));
        let root = t.alloc_table().unwrap();
        assert_eq!(map(&mut t, root, &identity(0x4000_0000, 0x1000, RW)), Err(MapError::OutOfTables));
    }

    #[test]
    fn range_leaving_its_canonical_half_is_rejected() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        // Low half: the second page would be at 2^48, which wraps to index 0.
        let m = Mapping { va: va(0x0000_ffff_ffff_f000), pa: PhysAddr::new(0x4000_0000), size: 0x2000, attrs: RW };
        assert_eq!(map(&mut t, root, &m), Err(MapError::OutOfRange));
        // High half: the end passes u64::MAX.
        let m = Mapping { va: va(0xffff_ffff_ffff_f000), ..m };
        assert_eq!(map(&mut t, root, &m), Err(MapError::OutOfRange));
        // Physical end past 2^48.
        let m = Mapping { va: va(0x4000_0000), pa: PhysAddr::new((1 << 48) - 0x1000), ..m };
        assert_eq!(map(&mut t, root, &m), Err(MapError::OutOfRange));
        assert_eq!(t.tables_used(), 1, "nothing was mapped");
        assert_eq!(translate(&t, root, va(0)), None);
    }

    /// Hands out one fixed table address, good or bad.
    struct FixedTables {
        inner: VecTables,
        next: u64,
    }

    impl TableMemory for FixedTables {
        fn alloc_table(&mut self) -> Option<PhysAddr> {
            Some(PhysAddr::new(self.next))
        }
        fn table(&self, pa: PhysAddr) -> &Table {
            self.inner.table(pa)
        }
        fn table_mut(&mut self, pa: PhysAddr) -> &mut Table {
            self.inner.table_mut(pa)
        }
    }

    #[test]
    fn unusable_table_address_is_rejected() {
        for (next, err) in [(0x1000_0800, MapError::Misaligned), (1 << 48, MapError::OutOfRange)] {
            let mut inner = VecTables::new(None);
            let root = inner.alloc_table().unwrap();
            let mut t = FixedTables { inner, next };
            assert_eq!(map(&mut t, root, &identity(0x4000_0000, 0x1000, RW)), Err(err));
            assert_eq!(t.inner.table(root)[0], 0, "no descriptor was written");
        }
    }

    #[test]
    fn indices_ignore_the_top_16_bits() {
        let mut t = VecTables::new(None);
        let root = t.alloc_table().unwrap();
        map(&mut t, root, &identity(0x4000_0000, 0x1000, RW)).unwrap();
        assert_eq!(
            translate(&t, root, va(0xffff_0000_4000_0010)).map(|x| x.0),
            Some(PhysAddr::new(0x4000_0010))
        );
    }
}
