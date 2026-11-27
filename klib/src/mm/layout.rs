// SPDX-License-Identifier: GPL-2.0-only
//! The boot memory layout ("layout B" of the study note): the kernel image
//! identity-mapped in `TTBR0`, all RAM linear-mapped in `TTBR1`. Linux
//! counterpart: `map_mem` and `map_kernel` in `arch/arm64/mm/mmu.c`, and
//! `__phys_to_virt`/`__virt_to_phys` in `arch/arm64/include/asm/memory.h`.

use crate::mm::addr::{PAGE_SIZE, PhysAddr, PhysRange, VirtAddr};
use crate::mm::map::{Access, Attrs, MapError, Mapping, MemType};

/// The start of the linear map, the bottom of the `TTBR1` half
/// (`_PAGE_OFFSET(48)`, `arch/arm64/include/asm/memory.h:45`).
pub const PAGE_OFFSET: u64 = 0xffff_0000_0000_0000;

/// The linear map is the lower half of the `TTBR1` range, up to `PAGE_END`
/// (`memory.h:39`, `:66`). The upper half is kept for later kernel
/// mappings, so it is never linear.
const LINEAR_SIZE: u64 = 1 << 47;

/// `PHYS_OFFSET` is the lowest RAM address rounded down to this, so linear
/// addresses and physical addresses share 2 MiB alignment and blocks fit.
pub const PHYS_OFFSET_ALIGN: u64 = 2 << 20;

/// The linear map: `va = PAGE_OFFSET + (pa - PHYS_OFFSET)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinearMap {
    phys_offset: PhysAddr,
}

impl LinearMap {
    pub fn new(lowest_ram: PhysAddr) -> Self {
        LinearMap { phys_offset: lowest_ram.align_down(PHYS_OFFSET_ALIGN) }
    }

    /// Linux: `PHYS_OFFSET` (`memstart_addr`).
    pub fn phys_offset(&self) -> PhysAddr {
        self.phys_offset
    }

    /// `pa - PHYS_OFFSET`, if `pa` is inside the linear map.
    fn offset(&self, pa: PhysAddr) -> Option<u64> {
        let off = pa.as_u64().checked_sub(self.phys_offset.as_u64())?;
        (off < LINEAR_SIZE).then_some(off)
    }

    /// Linux: `__phys_to_virt`.
    ///
    /// # Panics
    ///
    /// If `pa` is below `PHYS_OFFSET` or past the linear map. Linux gives a
    /// wrong address here (or a `VIRTUAL_BUG_ON` with `DEBUG_VIRTUAL`).
    pub fn phys_to_virt(&self, pa: PhysAddr) -> VirtAddr {
        match self.offset(pa).and_then(|off| VirtAddr::new(PAGE_OFFSET + off)) {
            Some(va) => va,
            None => panic!("phys_to_virt: {:#x} is outside the linear map", pa.as_u64()),
        }
    }

    /// Linux: `__virt_to_phys` for a linear address. `None` for any address
    /// outside the linear map.
    pub fn virt_to_phys(&self, va: VirtAddr) -> Option<PhysAddr> {
        let off = va.as_u64().checked_sub(PAGE_OFFSET)?;
        if off >= LINEAR_SIZE {
            return None;
        }
        self.phys_offset.checked_add(off)
    }
}

/// Where the kernel image lies. `data` includes `.bss` and the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageLayout {
    pub text: PhysRange,
    pub rodata: PhysRange,
    pub data: PhysRange,
}

/// Which translation table a mapping goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// `TTBR0`: the low half, identity-mapped.
    Low,
    /// `TTBR1`: the high half, the linear map.
    High,
}

/// Every mapping the kernel needs to run with the MMU on, in order, passed
/// to `emit` (layout B):
/// - `Low`, identity: `text` read-execute, `rodata` read-only, `data`
///   read-write, the DTB read-only and the UART read-write Device, the last
///   two rounded out to whole pages;
/// - `High`, linear: every RAM range read-write, rounded in to whole pages,
///   except the alias of `text` + `rodata`, which is read-only.
///
/// Empty pieces are left out. Nothing is emitted unless the layout passes
/// every check first:
/// - each image part lies inside one RAM range (`ImageOutsideRam`);
/// - the DTB and UART pages share no page with the image, the UART shares
///   none with the DTB, and the UART is not in RAM, which Linux's `ioremap`
///   also refuses (`Overlap`);
/// - all RAM fits in the linear map (`OutOfRange`).
pub fn boot_mappings(
    image: &ImageLayout,
    dtb: PhysRange,
    uart: PhysRange,
    ram: &[PhysRange],
    lm: &LinearMap,
    mut emit: impl FnMut(Root, Mapping) -> Result<(), MapError>,
) -> Result<(), MapError> {
    let parts = [image.text, image.rodata, image.data];
    if !parts.iter().all(|p| ram.iter().any(|r| r.contains(p))) {
        return Err(MapError::ImageOutsideRam);
    }
    let dtb = round_out(dtb)?;
    let uart = round_out(uart)?;
    for (a, b) in parts.iter().map(|p| (&dtb, p)).chain(parts.iter().map(|p| (&uart, p))) {
        check_apart(a, b)?;
    }
    check_apart(&uart, &dtb)?;
    for r in ram {
        check_apart(&uart, r)?;
    }
    for r in ram.iter().filter_map(round_in) {
        if lm.offset(r.start()).is_none() || lm.offset(PhysAddr::new(r.end().as_u64() - 1)).is_none() {
            return Err(MapError::OutOfRange);
        }
    }

    let normal = |access| Attrs { access, mem: MemType::Normal };
    let low = [
        (image.text, normal(Access::ReadExecute)),
        (image.rodata, normal(Access::ReadOnly)),
        (image.data, normal(Access::ReadWrite)),
        (dtb, normal(Access::ReadOnly)),
        (uart, Attrs { access: Access::ReadWrite, mem: MemType::Device }),
    ];
    for (range, attrs) in low.into_iter().filter(|(r, _)| !r.is_empty()) {
        let va = VirtAddr::new(range.start().as_u64()).ok_or(MapError::OutOfRange)?;
        emit(Root::Low, Mapping { va, pa: range.start(), size: range.size(), attrs })?;
    }

    let ro = hull(image.text, image.rodata);
    for r in ram.iter().filter_map(round_in) {
        let (s, e) = (r.start(), r.end());
        let pieces = [
            (s, e.min(ro.start()), Access::ReadWrite),
            (s.max(ro.start()), e.min(ro.end()), Access::ReadOnly),
            (s.max(ro.end()), e, Access::ReadWrite),
        ];
        for (start, end, access) in pieces.into_iter().filter(|(s, e, _)| s < e) {
            let size = end.as_u64() - start.as_u64();
            emit(Root::High, Mapping { va: lm.phys_to_virt(start), pa: start, size, attrs: normal(access) })?;
        }
    }
    Ok(())
}

/// `r` widened to whole pages.
fn round_out(r: PhysRange) -> Result<PhysRange, MapError> {
    let end = r.end().align_up(PAGE_SIZE).ok_or(MapError::OutOfRange)?;
    PhysRange::from_bounds(r.start().align_down(PAGE_SIZE), end).ok_or(MapError::OutOfRange)
}

/// The whole pages inside `r`, or `None` if there are none.
fn round_in(r: &PhysRange) -> Option<PhysRange> {
    let start = r.start().align_up(PAGE_SIZE)?;
    let end = r.end().align_down(PAGE_SIZE);
    PhysRange::from_bounds(start, end).filter(|r| !r.is_empty())
}

/// `Overlap` at the first shared byte if `a` and `b` overlap.
fn check_apart(a: &PhysRange, b: &PhysRange) -> Result<(), MapError> {
    if a.overlaps(b) {
        return Err(MapError::Overlap { pa: a.start().max(b.start()).as_u64() });
    }
    Ok(())
}

/// The smallest range holding both, ignoring an empty one.
fn hull(a: PhysRange, b: PhysRange) -> PhysRange {
    match (a.is_empty(), b.is_empty()) {
        (_, true) => a,
        (true, false) => b,
        (false, false) => {
            PhysRange::from_bounds(a.start().min(b.start()), a.end().max(b.end())).unwrap_or(a)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::aarch64::pgtable::testing::VecTables;
    use crate::arch::aarch64::pgtable::{TableMemory, map, translate};
    use crate::mm::addr::{PhysAddr, PhysRange, VirtAddr};
    use crate::mm::map::{Access, Attrs, MapError, Mapping, MemType};

    fn r(start: u64, end: u64) -> PhysRange {
        PhysRange::from_bounds(PhysAddr::new(start), PhysAddr::new(end)).unwrap()
    }

    fn qemu_image() -> ImageLayout {
        ImageLayout {
            text: r(0x4008_0000, 0x4009_0000),
            rodata: r(0x4009_0000, 0x4009_4000),
            data: r(0x4009_4000, 0x400a_8000),
        }
    }

    const DTB: (u64, u64) = (0x4800_0000, 0x4810_0000);
    const UART: (u64, u64) = (0x0900_0000, 0x0900_1000);
    const RAM: (u64, u64) = (0x4000_0000, 0x6000_0000);

    fn collect(
        image: &ImageLayout,
        dtb: PhysRange,
        uart: PhysRange,
        ram: &[PhysRange],
    ) -> Result<Vec<(Root, Mapping)>, MapError> {
        let lm = LinearMap::new(ram[0].start());
        let mut out = Vec::new();
        boot_mappings(image, dtb, uart, ram, &lm, |root, m| {
            out.push((root, m));
            Ok(())
        })?;
        Ok(out)
    }

    fn qemu() -> Result<Vec<(Root, Mapping)>, MapError> {
        collect(&qemu_image(), r(DTB.0, DTB.1), r(UART.0, UART.1), &[r(RAM.0, RAM.1)])
    }

    fn attrs(access: Access, mem: MemType) -> Attrs {
        Attrs { access, mem }
    }

    fn ident(start: u64, end: u64, a: Attrs) -> Mapping {
        Mapping { va: VirtAddr::new(start).unwrap(), pa: PhysAddr::new(start), size: end - start, attrs: a }
    }

    #[test]
    fn phys_to_virt_of_ram_base_is_page_offset() {
        let lm = LinearMap::new(PhysAddr::new(0x4000_0000));
        assert_eq!(lm.phys_offset(), PhysAddr::new(0x4000_0000));
        assert_eq!(lm.phys_to_virt(PhysAddr::new(0x4000_0000)), VirtAddr::new(0xffff_0000_0000_0000).unwrap());
        assert_eq!(lm.virt_to_phys(VirtAddr::new(0x4000_0000).unwrap()), None);
        assert_eq!(
            lm.virt_to_phys(VirtAddr::new(0xffff_0000_1234_5678).unwrap()),
            Some(PhysAddr::new(0x5234_5678))
        );
    }

    #[test]
    fn phys_offset_is_rounded_down_to_2_mib() {
        let lm = LinearMap::new(PhysAddr::new(0x4012_3000));
        assert_eq!(lm.phys_offset(), PhysAddr::new(0x4000_0000));
    }

    #[test]
    fn linear_map_is_the_lower_half_of_ttbr1() {
        let lm = LinearMap::new(PhysAddr::new(0x4000_0000));
        let top = PhysAddr::new(0x4000_0000 + (1 << 47) - 1);
        assert_eq!(lm.phys_to_virt(top), VirtAddr::new(0xffff_7fff_ffff_ffff).unwrap());
        assert_eq!(lm.virt_to_phys(VirtAddr::new(0xffff_8000_0000_0000).unwrap()), None);
        assert_eq!(lm.virt_to_phys(VirtAddr::new(0xffff_ff00_0000_0000).unwrap()), None);
    }

    #[test]
    #[should_panic(expected = "outside the linear map")]
    fn phys_to_virt_below_phys_offset_panics() {
        LinearMap::new(PhysAddr::new(0x4000_0000)).phys_to_virt(PhysAddr::new(0x3fff_f000));
    }

    #[test]
    #[should_panic(expected = "outside the linear map")]
    fn phys_to_virt_past_the_linear_map_panics() {
        LinearMap::new(PhysAddr::new(0x4000_0000)).phys_to_virt(PhysAddr::new(0x4000_0000 + (1 << 47)));
    }

    #[test]
    fn boot_mappings_follow_layout_b() {
        let all = qemu().unwrap();
        let low: Vec<Mapping> = all.iter().filter(|(root, _)| *root == Root::Low).map(|x| x.1).collect();
        let normal = |a| attrs(a, MemType::Normal);
        assert_eq!(
            low,
            vec![
                ident(0x4008_0000, 0x4009_0000, normal(Access::ReadExecute)),
                ident(0x4009_0000, 0x4009_4000, normal(Access::ReadOnly)),
                ident(0x4009_4000, 0x400a_8000, normal(Access::ReadWrite)),
                ident(DTB.0, DTB.1, normal(Access::ReadOnly)),
                ident(UART.0, UART.1, attrs(Access::ReadWrite, MemType::Device)),
            ]
        );

        let lm = LinearMap::new(PhysAddr::new(RAM.0));
        let high: Vec<Mapping> = all.iter().filter(|(root, _)| *root == Root::High).map(|x| x.1).collect();
        // High covers all of RAM, in order, at the linear address.
        let mut next = RAM.0;
        for m in &high {
            assert_eq!(m.pa, PhysAddr::new(next));
            assert_eq!(m.va, lm.phys_to_virt(m.pa));
            assert_eq!(m.attrs.mem, MemType::Normal);
            next += m.size;
        }
        assert_eq!(next, RAM.1);
        // The alias of text + rodata is read-only, the rest is read-write.
        for m in &high {
            let range = PhysRange::new(m.pa, m.size).unwrap();
            let image_ro = r(0x4008_0000, 0x4009_4000);
            if range.overlaps(&image_ro) {
                assert_eq!(range, image_ro);
                assert_eq!(m.attrs.access, Access::ReadOnly);
            } else {
                assert_eq!(m.attrs.access, Access::ReadWrite);
            }
        }
        for (root, m) in &all {
            assert!(!(m.attrs.access == Access::ReadWrite && m.attrs.access == Access::ReadExecute));
            if *root == Root::High {
                assert_ne!(m.attrs.access, Access::ReadExecute, "{m:?} is executable");
            }
        }
    }

    #[test]
    fn boot_mappings_map_cleanly_into_two_roots() {
        let mut t = VecTables::new(None);
        let (low, high) = (t.alloc_table().unwrap(), t.alloc_table().unwrap());
        let lm = LinearMap::new(PhysAddr::new(RAM.0));
        let ram = [r(RAM.0, RAM.1)];
        boot_mappings(&qemu_image(), r(DTB.0, DTB.1), r(UART.0, UART.1), &ram, &lm, |root, m| {
            map(&mut t, if root == Root::Low { low } else { high }, &m)
        })
        .unwrap();
        let at = |root, a: u64| translate(&t, root, VirtAddr::new(a).unwrap()).map(|x| x.0.as_u64());
        assert_eq!(at(low, 0x4008_0000), Some(0x4008_0000));
        assert_eq!(at(low, 0x400a_7ff8), Some(0x400a_7ff8));
        assert_eq!(at(low, 0x400a_8000), None, "past the image");
        assert_eq!(at(low, 0x0900_0000), Some(0x0900_0000));
        assert_eq!(at(high, 0xffff_0000_0008_0000), Some(0x4008_0000));
        assert_eq!(at(high, 0xffff_0000_1fff_fff8), Some(0x5fff_fff8));
    }

    #[test]
    fn image_outside_ram_is_an_error() {
        let image = ImageLayout { data: r(0x6000_0000, 0x6001_0000), ..qemu_image() };
        assert_eq!(
            collect(&image, r(DTB.0, DTB.1), r(UART.0, UART.1), &[r(RAM.0, RAM.1)]),
            Err(MapError::ImageOutsideRam)
        );
        let image = ImageLayout { text: r(0x3ff0_0000, 0x3ff1_0000), ..qemu_image() };
        assert_eq!(
            collect(&image, r(DTB.0, DTB.1), r(UART.0, UART.1), &[r(RAM.0, RAM.1)]),
            Err(MapError::ImageOutsideRam)
        );
    }

    #[test]
    fn dtb_is_rounded_out_to_pages() {
        let all = collect(&qemu_image(), r(0x4800_0010, 0x4800_1010), r(UART.0, UART.1), &[r(RAM.0, RAM.1)]).unwrap();
        assert_eq!(all[3].1, ident(0x4800_0000, 0x4800_2000, attrs(Access::ReadOnly, MemType::Normal)));
    }

    #[test]
    fn dtb_sharing_a_page_with_the_image_is_rejected() {
        // The last page of .data must not turn read-only.
        assert_eq!(
            collect(&qemu_image(), r(0x400a_7f00, 0x400a_9000), r(UART.0, UART.1), &[r(RAM.0, RAM.1)]),
            Err(MapError::Overlap { pa: 0x400a_7000 })
        );
        assert!(collect(&qemu_image(), r(0x400a_8000, 0x400a_9000), r(UART.0, UART.1), &[r(RAM.0, RAM.1)]).is_ok());
    }

    #[test]
    fn uart_inside_ram_or_image_is_rejected() {
        // Linux's ioremap refuses RAM; a Device alias of RAM mismatches attributes.
        assert_eq!(
            collect(&qemu_image(), r(DTB.0, DTB.1), r(0x5000_0000, 0x5000_1000), &[r(RAM.0, RAM.1)]),
            Err(MapError::Overlap { pa: 0x5000_0000 })
        );
        assert_eq!(
            collect(&qemu_image(), r(DTB.0, DTB.1), r(0x4009_0000, 0x4009_1000), &[r(RAM.0, RAM.1)]),
            Err(MapError::Overlap { pa: 0x4009_0000 })
        );
    }

    #[test]
    fn dtb_and_uart_sharing_a_page_is_rejected() {
        // A DTB outside RAM (in flash, say) next to the UART.
        assert_eq!(
            collect(&qemu_image(), r(0x0900_0800, 0x0900_2000), r(UART.0, UART.1), &[r(RAM.0, RAM.1)]),
            Err(MapError::Overlap { pa: 0x0900_0000 })
        );
    }

    #[test]
    fn ram_outside_the_linear_map_is_rejected() {
        let far = 0x4000_0000 + (1 << 47);
        assert_eq!(
            collect(&qemu_image(), r(DTB.0, DTB.1), r(UART.0, UART.1), &[r(RAM.0, RAM.1), r(far - 0x1000, far + 0x1000)]),
            Err(MapError::OutOfRange)
        );
        let lm = LinearMap::new(PhysAddr::new(RAM.0));
        let below = [r(0x3000_0000, 0x3000_1000), r(RAM.0, RAM.1)];
        let res = boot_mappings(&qemu_image(), r(DTB.0, DTB.1), r(UART.0, UART.1), &below, &lm, |_, _| Ok(()));
        assert_eq!(res, Err(MapError::OutOfRange));
    }

    #[test]
    fn ram_ranges_are_rounded_inward_to_pages() {
        let all = collect(&qemu_image(), r(DTB.0, DTB.1), r(UART.0, UART.1), &[r(0x4000_0800, 0x5fff_ff00)]).unwrap();
        let high: Vec<Mapping> = all.iter().filter(|(root, _)| *root == Root::High).map(|x| x.1).collect();
        assert_eq!(high.first().unwrap().pa, PhysAddr::new(0x4000_1000));
        let last = high.last().unwrap();
        assert_eq!(last.pa.as_u64() + last.size, 0x5fff_f000);
    }

    #[test]
    fn emit_errors_are_passed_back() {
        let lm = LinearMap::new(PhysAddr::new(RAM.0));
        let res = boot_mappings(&qemu_image(), r(DTB.0, DTB.1), r(UART.0, UART.1), &[r(RAM.0, RAM.1)], &lm, |_, _| {
            Err(MapError::OutOfTables)
        });
        assert_eq!(res, Err(MapError::OutOfTables));
    }
}
