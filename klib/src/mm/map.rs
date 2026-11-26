// SPDX-License-Identifier: GPL-2.0-only
//! What a mapping is, independent of the CPU's table format. Linux
//! counterpart: the `pgprot_t` values (`PAGE_KERNEL`, `PAGE_KERNEL_RO`,
//! `PAGE_KERNEL_ROX`, `PROT_DEVICE_nGnRE`) passed to `__create_pgd_mapping`.

use crate::mm::addr::{PhysAddr, VirtAddr};

/// What the kernel may do with a mapping. There is no writable and
/// executable choice: W^X holds by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
    ReadExecute,
}

/// The memory type: cached RAM, or device registers (nGnRE).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemType {
    Normal,
    Device,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attrs {
    pub access: Access,
    pub mem: MemType,
}

/// Map `[va, va + size)` to `[pa, pa + size)` with `attrs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub va: VirtAddr,
    pub pa: PhysAddr,
    pub size: u64,
    pub attrs: Attrs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapError {
    /// `va`, `pa` or `size` is not a multiple of the page size, or a block
    /// address is not a multiple of the block size.
    Misaligned,
    /// The table memory has no table left.
    OutOfTables,
    /// The entry for `va` is live and the change needs break-before-make,
    /// which boot mapping never does (Linux: the `BUG_ON` in `mmu.c`).
    BreakBeforeMake { va: u64 },
    /// Device memory can't be executable.
    ExecutableDevice,
    /// A part of the kernel image is not inside one RAM range.
    ImageOutsideRam,
    /// The virtual range leaves its half of the address space, a physical
    /// address passes 48 bits, or RAM lies outside the linear map.
    OutOfRange,
    /// Two boot mappings would share the page at `pa` with different
    /// attributes, or the UART lies in RAM.
    Overlap { pa: u64 },
}
