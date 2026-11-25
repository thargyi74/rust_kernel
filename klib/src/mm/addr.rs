// SPDX-License-Identifier: GPL-2.0-only
//! Typed addresses, so a physical address can't be used as a virtual one by
//! mistake. Linux counterpart: `phys_addr_t`, `__pa`/`__va` and the PFN
//! helpers in `include/linux/pfn.h`.

/// log2 of the page size (Linux: `PAGE_SHIFT` with 4 KiB pages).
pub const PAGE_SHIFT: u32 = 12;
pub const PAGE_SIZE: u64 = 1 << PAGE_SHIFT;
/// One past the highest physical address we accept: 48-bit physical
/// addresses (Linux: `MAX_MEMBLOCK_ADDR` + 1 with `PHYS_MASK_SHIFT` = 48).
pub const MAX_PHYS: u64 = 1 << 48;

/// A physical address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(u64);

/// A canonical virtual address: bits 63:48 are all 0 or all 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(u64);

/// A page frame number: a physical address divided by [`PAGE_SIZE`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pfn(u64);

impl PhysAddr {
    pub const fn new(addr: u64) -> Self {
        PhysAddr(addr)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, n: u64) -> Option<Self> {
        self.0.checked_add(n).map(PhysAddr)
    }

    /// Rounds down to a multiple of `align`, which must be a power of two.
    pub fn align_down(self, align: u64) -> Self {
        PhysAddr(self.0 & !align.wrapping_sub(1))
    }

    /// Rounds up to a multiple of `align`, which must be a power of two.
    /// `None` if that passes `u64::MAX`.
    pub fn align_up(self, align: u64) -> Option<Self> {
        Some(PhysAddr(self.0.checked_add(align.wrapping_sub(1))?).align_down(align))
    }

    /// Whether this is a multiple of `align`, which must be a power of two.
    pub fn is_aligned(self, align: u64) -> bool {
        self.0 & align.wrapping_sub(1) == 0
    }

    /// The frame this address lies in (Linux: `PHYS_PFN`).
    pub fn pfn(self) -> Pfn {
        Pfn(self.0 >> PAGE_SHIFT)
    }
}

impl VirtAddr {
    /// `None` unless `addr` is canonical for 48-bit virtual addresses.
    pub fn new(addr: u64) -> Option<Self> {
        match addr >> 48 {
            0 | 0xffff => Some(VirtAddr(addr)),
            _ => None,
        }
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl Pfn {
    pub const fn new(pfn: u64) -> Self {
        Pfn(pfn)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// The address of the frame's first byte (Linux: `PFN_PHYS`).
    pub fn addr(self) -> PhysAddr {
        PhysAddr(self.0 << PAGE_SHIFT)
    }
}

/// A half-open physical range `[start, end)`, always with `start <= end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysRange {
    start: PhysAddr,
    end: PhysAddr,
}

impl PhysRange {
    /// The empty range at address 0, to fill fixed-size arrays.
    pub const EMPTY: PhysRange = PhysRange { start: PhysAddr(0), end: PhysAddr(0) };

    /// `[start, start + size)`, or `None` if the end passes `u64::MAX`.
    pub fn new(start: PhysAddr, size: u64) -> Option<Self> {
        Some(PhysRange { start, end: start.checked_add(size)? })
    }

    /// `[start, end)`, or `None` if `end < start`.
    pub fn from_bounds(start: PhysAddr, end: PhysAddr) -> Option<Self> {
        (start <= end).then_some(PhysRange { start, end })
    }

    pub fn start(&self) -> PhysAddr {
        self.start
    }

    pub fn end(&self) -> PhysAddr {
        self.end
    }

    pub fn size(&self) -> u64 {
        self.end.0 - self.start.0
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Whether the two share at least one byte. An empty range overlaps
    /// nothing.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Whether every byte of `other` is also in `self`.
    pub fn contains(&self, other: &Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virt_addr_rejects_non_canonical() {
        assert_eq!(VirtAddr::new(0x0001_0000_0000_0000), None);
        assert!(VirtAddr::new(0xffff_0000_0000_0000).is_some());
        assert!(VirtAddr::new(0x4000_0000).is_some());
    }

    #[test]
    fn phys_range_new_rejects_overflow() {
        assert_eq!(PhysRange::new(PhysAddr::new(u64::MAX - 1), 2), None);
        assert!(PhysRange::new(PhysAddr::new(u64::MAX - 1), 1).is_some());
        assert_eq!(PhysRange::from_bounds(PhysAddr::new(2), PhysAddr::new(1)), None);
    }

    #[test]
    fn phys_addr_alignment_and_pfn() {
        let a = PhysAddr::new(0x4000_1234);
        assert_eq!(a.align_down(PAGE_SIZE), PhysAddr::new(0x4000_1000));
        assert_eq!(a.align_up(PAGE_SIZE), Some(PhysAddr::new(0x4000_2000)));
        assert_eq!(PhysAddr::new(u64::MAX).align_up(PAGE_SIZE), None);
        assert!(!a.is_aligned(PAGE_SIZE) && PhysAddr::new(0x4000_0000).is_aligned(PAGE_SIZE));
        assert_eq!(a.pfn(), Pfn::new(0x4_0001));
        assert_eq!(Pfn::new(0x4_0001).addr(), PhysAddr::new(0x4000_1000));
    }

    #[test]
    fn phys_range_overlap_and_contain() {
        let r = |s, e| PhysRange::from_bounds(PhysAddr::new(s), PhysAddr::new(e)).unwrap();
        assert!(r(0, 0x2000).overlaps(&r(0x1000, 0x3000)));
        assert!(!r(0, 0x1000).overlaps(&r(0x1000, 0x2000)));
        assert!(r(0, 0x3000).contains(&r(0x1000, 0x2000)));
        assert!(!r(0, 0x1800).contains(&r(0x1000, 0x2000)));
        assert_eq!(r(0x1000, 0x3000).size(), 0x2000);
        assert!(r(5, 5).is_empty());
    }
}
