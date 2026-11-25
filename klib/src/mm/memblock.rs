// SPDX-License-Identifier: GPL-2.0-only
//! The boot-time memory manager: which physical ranges exist, which are
//! taken, and a simple top-down allocator. Linux counterpart: `mm/memblock.c`.

//!
//! Simplified against Linux: one fixed array of [`MAX_REGIONS`] ranges each
//! for memory and reservations (Linux doubles its arrays when they fill up),
//! no NUMA node ids and no region flags.

use crate::fdt::{Fdt, FdtError};
use crate::mm::addr::{MAX_PHYS, PAGE_SIZE, PhysAddr, PhysRange};

/// Ranges per array (Linux starts with `INIT_MEMBLOCK_REGIONS` = 128).
pub const MAX_REGIONS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemblockError {
    /// A region array already holds [`MAX_REGIONS`] ranges.
    Full,
    /// The device tree describes no usable memory.
    NoMemory,
    /// A memory node has a bad `reg`.
    Fdt(FdtError),
}

/// A memory range from the device tree, made usable the way
/// `early_init_dt_add_memory_arch` does (`drivers/of/fdt.c:1159-1199`): the
/// start is rounded up and the size down to whole pages, and the range is
/// clipped at [`MAX_PHYS`]. `None` if nothing is left.
pub fn dt_memory_range(base: u64, size: u64) -> Option<PhysRange> {
    let start = PhysAddr::new(base).align_up(PAGE_SIZE)?;
    let end = PhysAddr::new(base.saturating_add(size).min(MAX_PHYS)).align_down(PAGE_SIZE);
    if start >= end {
        return None;
    }
    PhysRange::from_bounds(start, end)
}

/// A sorted array of disjoint, non-touching ranges (Linux:
/// `struct memblock_type`).
struct Regions {
    ranges: [PhysRange; MAX_REGIONS],
    len: usize,
}

impl Regions {
    const fn new() -> Self {
        Regions { ranges: [PhysRange::EMPTY; MAX_REGIONS], len: 0 }
    }

    fn as_slice(&self) -> &[PhysRange] {
        self.ranges.get(..self.len).unwrap_or(&[])
    }

    /// Adds `r`, merging it with every range it overlaps or touches, as
    /// `memblock_add_range` + `memblock_merge_regions` do.
    fn insert(&mut self, r: PhysRange) -> Result<(), MemblockError> {
        if r.is_empty() {
            return Ok(());
        }
        let list = self.as_slice();
        // [lo, hi) are the ranges that overlap or touch `r`.
        let lo = list.iter().position(|x| x.end() >= r.start()).unwrap_or(list.len());
        let hi = lo + list.iter().skip(lo).take_while(|x| x.start() <= r.end()).count();
        if lo == hi {
            if self.len == MAX_REGIONS {
                return Err(MemblockError::Full);
            }
            self.ranges.copy_within(lo..self.len, lo + 1);
            self.ranges[lo] = r;
            self.len += 1;
        } else {
            let start = r.start().min(list[lo].start());
            let end = r.end().max(list[hi - 1].end());
            self.ranges[lo] = PhysRange::from_bounds(start, end).unwrap_or(r);
            self.ranges.copy_within(hi..self.len, lo + 1);
            self.len -= hi - lo - 1;
        }
        Ok(())
    }
}

/// Physical memory and reservations, before the page allocator exists
/// (Linux: `struct memblock`).
pub struct Memblock {
    memory: Regions,
    reserved: Regions,
}

impl Default for Memblock {
    fn default() -> Self {
        Self::new()
    }
}

impl Memblock {
    pub const fn new() -> Self {
        Memblock { memory: Regions::new(), reserved: Regions::new() }
    }

    /// Adds RAM (Linux: `memblock_add`).
    pub fn add(&mut self, r: PhysRange) -> Result<(), MemblockError> {
        self.memory.insert(r)
    }

    /// Marks a range as taken (Linux: `memblock_reserve`). It need not lie
    /// inside memory.
    pub fn reserve(&mut self, r: PhysRange) -> Result<(), MemblockError> {
        self.reserved.insert(r)
    }

    pub fn memory(&self) -> &[PhysRange] {
        self.memory.as_slice()
    }

    pub fn reserved(&self) -> &[PhysRange] {
        self.reserved.as_slice()
    }

    /// Memory minus reservations, ascending (Linux:
    /// `for_each_free_mem_range`).
    pub fn free_ranges(&self) -> impl Iterator<Item = PhysRange> + '_ {
        let (memory, reserved) = (self.memory(), self.reserved());
        let mut index = 0;
        // Where the unvisited part of memory[index] starts.
        let mut cursor: Option<PhysAddr> = None;
        core::iter::from_fn(move || {
            loop {
                let m = memory.get(index)?;
                let start = cursor.map_or(m.start(), |c| c.max(m.start()));
                if start >= m.end() {
                    index += 1;
                    cursor = None;
                    continue;
                }
                // Ranges are sorted and disjoint, so their ends are sorted too.
                match reserved.iter().find(|x| x.end() > start) {
                    Some(x) if x.start() <= start => cursor = Some(x.end()),
                    Some(x) if x.start() < m.end() => {
                        cursor = Some(x.end());
                        return PhysRange::from_bounds(start, x.start());
                    }
                    _ => {
                        index += 1;
                        cursor = None;
                        return PhysRange::from_bounds(start, m.end());
                    }
                }
            }
        })
    }

    /// Bytes of memory, reserved or not (Linux: `memblock_phys_mem_size`).
    pub fn total_memory(&self) -> u64 {
        self.memory().iter().map(PhysRange::size).sum()
    }

    /// Finds the highest `align`-aligned free block of `size` bytes and
    /// reserves it, as `memblock_phys_alloc` does with its top-down search
    /// (`mm/memblock.c:270-291`). `None` if nothing fits, if `size` is 0, if
    /// `align` is not a power of two, or if the reservation array is full.
    /// Address 0 is a valid result.
    pub fn alloc(&mut self, size: u64, align: u64) -> Option<PhysAddr> {
        if size == 0 || !align.is_power_of_two() {
            return None;
        }
        // Free ranges come in ascending order, so the last fit is the highest.
        let found = self
            .free_ranges()
            .filter_map(|f| {
                let cand = PhysAddr::new(f.end().as_u64().checked_sub(size)?).align_down(align);
                (cand >= f.start()).then_some(cand)
            })
            .last()?;
        self.reserve(PhysRange::new(found, size)?).ok()?;
        Some(found)
    }

    /// Fills a memblock from the device tree's memory nodes, then reserves
    /// `reserve` (the kernel image and the device tree) and every
    /// `/memreserve/` entry, before anything can be allocated. Linux does
    /// this in `early_init_dt_scan_memory` and `arm64_memblock_init`.
    pub fn from_device_tree(fdt: &Fdt<'_>, reserve: &[PhysRange]) -> Result<Memblock, MemblockError> {
        let mut mb = Memblock::new();
        for node in fdt.memory_nodes() {
            for (base, size) in node.reg().map_err(MemblockError::Fdt)? {
                if let Some(r) = dt_memory_range(base, size) {
                    mb.add(r)?;
                }
            }
        }
        if mb.memory().is_empty() {
            return Err(MemblockError::NoMemory);
        }
        for &r in reserve {
            mb.reserve(r)?;
        }
        for (base, size) in fdt.mem_reservations() {
            let base = PhysAddr::new(base);
            // An entry past the end of the address space keeps everything above `base`.
            let r = PhysRange::new(base, size).or(PhysRange::from_bounds(base, PhysAddr::new(u64::MAX)));
            mb.reserve(r.unwrap_or(PhysRange::EMPTY))?;
        }
        Ok(mb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fdt::testing::Builder;
    use crate::fdt::Fdt;
    use crate::mm::addr::{PhysAddr, PhysRange};
    use crate::prandom::XorShift64;

    const QEMU: &[u8] = include_bytes!("../../testdata/qemu-virt-512m.dtb");

    fn r(s: u64, e: u64) -> PhysRange {
        PhysRange::from_bounds(PhysAddr::new(s), PhysAddr::new(e)).unwrap()
    }

    #[test]
    fn add_keeps_ranges_sorted_and_merges_neighbours() {
        let mut m = Memblock::new();
        for x in [r(0x5000, 0x6000), r(0x1000, 0x2000), r(0x2000, 0x3000)] {
            m.add(x).unwrap();
        }
        assert_eq!(m.memory(), &[r(0x1000, 0x3000), r(0x5000, 0x6000)]);
    }

    #[test]
    fn add_overlapping_range_merges_into_one() {
        let mut m = Memblock::new();
        m.add(r(0x1000, 0x3000)).unwrap();
        m.add(r(0x2000, 0x4000)).unwrap();
        assert_eq!(m.memory(), &[r(0x1000, 0x4000)]);
    }

    #[test]
    fn add_range_spanning_several_merges_them_all() {
        let mut m = Memblock::new();
        for x in [r(0x1000, 0x2000), r(0x3000, 0x4000), r(0x5000, 0x6000), r(0x8000, 0x9000)] {
            m.add(x).unwrap();
        }
        m.add(r(0x1800, 0x5800)).unwrap();
        assert_eq!(m.memory(), &[r(0x1000, 0x6000), r(0x8000, 0x9000)]);
    }

    #[test]
    fn free_ranges_subtract_reserved() {
        let mut m = Memblock::new();
        m.add(r(0x4000_0000, 0x6000_0000)).unwrap();
        m.reserve(r(0x4008_0000, 0x4010_0000)).unwrap();
        m.reserve(r(0x4800_0000, 0x4810_0000)).unwrap();
        assert_eq!(
            m.free_ranges().collect::<Vec<_>>(),
            vec![r(0x4000_0000, 0x4008_0000), r(0x4010_0000, 0x4800_0000), r(0x4810_0000, 0x6000_0000)]
        );
    }

    #[test]
    fn free_ranges_handle_reservations_across_memory_ranges() {
        let mut m = Memblock::new();
        m.add(r(0x1000, 0x3000)).unwrap();
        m.add(r(0x5000, 0x8000)).unwrap();
        m.reserve(r(0, 0x1800)).unwrap();
        m.reserve(r(0x2800, 0x6000)).unwrap();
        m.reserve(r(0x7000, 0x9000)).unwrap();
        assert_eq!(m.free_ranges().collect::<Vec<_>>(), vec![r(0x1800, 0x2800), r(0x6000, 0x7000)]);
        assert_eq!(m.total_memory(), 0x5000);
    }

    #[test]
    fn alloc_is_top_down_and_aligned() {
        let mut m = Memblock::new();
        m.add(r(0x4000_0000, 0x6000_0000)).unwrap();
        assert_eq!(m.alloc(0x1000, 0x1000), Some(PhysAddr::new(0x5fff_f000)));
        assert_eq!(m.alloc(0x10, 0x20_0000), Some(PhysAddr::new(0x5fe0_0000)));
    }

    #[test]
    fn alloc_can_return_physical_address_zero() {
        let mut m = Memblock::new();
        m.add(r(0, 0x2000)).unwrap();
        assert_eq!(m.alloc(0x1000, 0x1000), Some(PhysAddr::new(0x1000)));
        assert_eq!(m.alloc(0x1000, 0x1000), Some(PhysAddr::new(0)));
        assert_eq!(m.alloc(0x1000, 0x1000), None);
    }

    #[test]
    fn alloc_returns_none_when_nothing_fits() {
        let mut m = Memblock::new();
        m.add(r(0x1000, 0x3000)).unwrap();
        m.reserve(r(0x1800, 0x1900)).unwrap();
        assert_eq!(m.alloc(0x2000, 1), None, "bigger than any free range");
        assert_eq!(m.alloc(0x1000, 0x4000), None, "fits only unaligned");
        assert_eq!(m.alloc(0x10, 3), None, "alignment not a power of two");
        assert_eq!(m.alloc(0x10, 0), None, "alignment zero");
        assert_eq!(m.reserved(), &[r(0x1800, 0x1900)], "failed allocs reserve nothing");
    }

    #[test]
    fn alloc_never_returns_reserved_or_already_allocated_memory() {
        let memory = r(0x4000_0000, 0x4100_0000);
        let mut m = Memblock::new();
        m.add(memory).unwrap();
        let mut rng = XorShift64::new(1);
        let mut reserved = Vec::new();
        for _ in 0..8 {
            let start = 0x4000_0000 + rng.below(0x100_0000);
            let x = r(start, (start + 1 + rng.below(0x2_0000)).min(0x4100_0000));
            m.reserve(x).unwrap();
            reserved.push(x);
        }
        let mut taken: Vec<PhysRange> = Vec::new();
        for _ in 0..2000 {
            let size = 1 + rng.below(0x8000);
            let align = 1 << rng.below(17);
            if let Some(a) = m.alloc(size, align) {
                let got = PhysRange::new(a, size).unwrap();
                assert!(a.is_aligned(align), "{a:?} not aligned to {align:#x}");
                assert!(memory.contains(&got), "{got:?} outside memory");
                assert!(!reserved.iter().any(|x| x.overlaps(&got)), "{got:?} overlaps a reservation");
                assert!(!taken.iter().any(|x| x.overlaps(&got)), "{got:?} overlaps an earlier result");
                taken.push(got);
            }
        }
        assert!(taken.len() >= 100, "only {} allocations succeeded", taken.len());
    }

    #[test]
    fn full_region_array_is_an_error() {
        let mut m = Memblock::new();
        for i in 0..MAX_REGIONS as u64 {
            m.add(r(i * 0x2000, i * 0x2000 + 0x1000)).unwrap();
        }
        let next = MAX_REGIONS as u64 * 0x2000;
        assert_eq!(m.add(r(next, next + 0x1000)), Err(MemblockError::Full));
        assert_eq!(m.memory().len(), MAX_REGIONS);
    }

    #[test]
    fn dt_memory_range_rounds_and_clips() {
        assert_eq!(dt_memory_range(0x4000_0800, 0x3000), Some(r(0x4000_1000, 0x4000_3000)));
        assert_eq!(dt_memory_range(0x4000_0800, 0x700), None);
        assert_eq!(dt_memory_range(0xffff_ffff_0000, 0x10_0000), Some(r(0xffff_ffff_0000, 1 << 48)));
        assert_eq!(dt_memory_range(u64::MAX - 0xfff, 0x2000), None);
    }

    #[test]
    fn from_device_tree_reads_qemu_memory_and_reserves() {
        let fdt = Fdt::new(QEMU).unwrap();
        let image = r(0x4008_0000, 0x4010_0000);
        let dtb = r(0x4800_0000, 0x4810_0000);
        let m = Memblock::from_device_tree(&fdt, &[image, dtb]).unwrap();
        assert_eq!(m.memory(), &[r(0x4000_0000, 0x6000_0000)]);
        assert_eq!(m.reserved(), &[image, dtb]);
    }

    #[test]
    fn from_device_tree_reserves_memreserve_entries() {
        let mut b = Builder::new();
        b.reserve(0x4800_0000, 0x1000).begin("").prop_u32("#address-cells", &[1]).prop_u32("#size-cells", &[1]);
        b.begin("memory@40000000").prop_str("device_type", "memory").prop_u32("reg", &[0x4000_0000, 0x1000_0000]);
        b.end().end();
        let blob = b.finish();
        let m = Memblock::from_device_tree(&Fdt::new(&blob).unwrap(), &[]).unwrap();
        assert_eq!(m.reserved(), &[r(0x4800_0000, 0x4800_1000)]);
    }

    #[test]
    fn from_device_tree_without_memory_node_is_no_memory() {
        let mut b = Builder::new();
        b.begin("").prop_u32("#address-cells", &[1]).prop_u32("#size-cells", &[1]).end();
        let blob = b.finish();
        let fdt = Fdt::new(&blob).unwrap();
        assert!(matches!(Memblock::from_device_tree(&fdt, &[]), Err(MemblockError::NoMemory)));
    }
}
