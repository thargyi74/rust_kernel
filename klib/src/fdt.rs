// SPDX-License-Identifier: GPL-2.0-only
//! Flattened device tree (FDT) header check.
//!
//! Mirrors `fdt_check_header()` in Linux's `scripts/dtc/libfdt/fdt.c`, but
//! for the version-17 header layout only. libfdt accepts `version >= 2` with
//! `last_comp_version <= 17`; we require `version >= 17` (and the same
//! `last_comp_version <= 17`), because QEMU and every modern `dtc` emit 17 and
//! older header layouts are shorter.
//!
//! The header is untrusted input (it arrives in a register at boot), so every
//! offset and size is bounds-checked with overflow-safe arithmetic: a hostile
//! header must produce an error, never a panic.

/// `FDT_MAGIC` in `scripts/dtc/libfdt/fdt.h`.
pub const FDT_MAGIC: u32 = 0xd00d_feed;

/// Size of the version-17 header: ten big-endian `u32` fields
/// (`FDT_V17_SIZE` in `fdt.h`).
pub const FDT_V17_HEADER_SIZE: usize = 40;

/// Largest device tree the kernel accepts (`MAX_FDT_SIZE` in
/// `arch/arm64/include/asm/boot.h`).
pub const MAX_FDT_SIZE: usize = 2 * 1024 * 1024;

/// Oldest `version` we parse, and newest `last_comp_version` we understand
/// (`FDT_LAST_SUPPORTED_VERSION` = 0x11 in libfdt).
const SUPPORTED_VERSION: u32 = 17;

/// `struct fdt_header` from `fdt.h`, minus `magic` (always `FDT_MAGIC` once parsed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdtHeader {
    pub totalsize: u32,
    pub off_dt_struct: u32,
    pub off_dt_strings: u32,
    pub off_mem_rsvmap: u32,
    pub version: u32,
    pub last_comp_version: u32,
    pub boot_cpuid_phys: u32,
    pub size_dt_strings: u32,
    pub size_dt_struct: u32,
}

/// Why a header was rejected. Variants follow libfdt's `-FDT_ERR_*` codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdtError {
    /// `FDT_ERR_TRUNCATED`: too short, or a block lies outside `totalsize`.
    Truncated,
    /// `FDT_ERR_BADMAGIC`, carrying the magic that was found.
    BadMagic(u32),
    /// `FDT_ERR_BADVERSION`.
    BadVersion { version: u32, last_comp_version: u32 },
    /// `totalsize` exceeds [`MAX_FDT_SIZE`].
    TooLarge(u32),
    /// `FDT_ERR_ALIGNMENT`: a block offset is misaligned.
    Alignment,
}

impl FdtHeader {
    /// Validates the header at the start of `blob`, which must hold the whole
    /// device tree (`totalsize` bytes). Checks run in libfdt's order.
    pub fn parse(blob: &[u8]) -> Result<FdtHeader, FdtError> {
        if blob.len() < FDT_V17_HEADER_SIZE {
            return Err(FdtError::Truncated);
        }
        let field = |i: usize| {
            let b = &blob[i * 4..i * 4 + 4];
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        };

        let magic = field(0);
        if magic != FDT_MAGIC {
            return Err(FdtError::BadMagic(magic));
        }

        let h = FdtHeader {
            totalsize: field(1),
            off_dt_struct: field(2),
            off_dt_strings: field(3),
            off_mem_rsvmap: field(4),
            version: field(5),
            last_comp_version: field(6),
            boot_cpuid_phys: field(7),
            size_dt_strings: field(8),
            size_dt_struct: field(9),
        };

        if h.version < SUPPORTED_VERSION
            || h.last_comp_version > SUPPORTED_VERSION
            || h.version < h.last_comp_version
        {
            return Err(FdtError::BadVersion {
                version: h.version,
                last_comp_version: h.last_comp_version,
            });
        }

        let total = h.totalsize as usize;
        if total < FDT_V17_HEADER_SIZE {
            return Err(FdtError::Truncated);
        }
        if total > MAX_FDT_SIZE {
            return Err(FdtError::TooLarge(h.totalsize));
        }
        if total > blob.len() {
            return Err(FdtError::Truncated);
        }

        // The memory-reservation map holds u64 pairs; the structure block holds
        // u32 tokens (FDT_TAGSIZE).
        if !h.off_mem_rsvmap.is_multiple_of(8) || !h.off_dt_struct.is_multiple_of(4) {
            return Err(FdtError::Alignment);
        }

        if !offset_in_bounds(h.off_mem_rsvmap, h.totalsize)
            || !block_in_bounds(h.off_dt_struct, h.size_dt_struct, h.totalsize)
            || !block_in_bounds(h.off_dt_strings, h.size_dt_strings, h.totalsize)
        {
            return Err(FdtError::Truncated);
        }

        Ok(h)
    }
}

/// libfdt's `check_off_()`: `off` lies in `[header size, totalsize]`.
fn offset_in_bounds(off: u32, totalsize: u32) -> bool {
    off as usize >= FDT_V17_HEADER_SIZE && off <= totalsize
}

/// libfdt's `check_block_()`: `[off, off + size)` lies inside the blob after
/// the header, without wrapping around.
fn block_in_bounds(off: u32, size: u32, totalsize: u32) -> bool {
    match off.checked_add(size) {
        Some(end) => offset_in_bounds(off, totalsize) && offset_in_bounds(end, totalsize),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: [u32; 10] = [FDT_MAGIC, 256, 56, 200, 40, 17, 16, 0, 56, 144];

    /// The ten header fields big-endian, zero-padded to `len` bytes.
    fn blob(f: [u32; 10], len: usize) -> Vec<u8> {
        let mut v: Vec<u8> = f.iter().flat_map(|x| x.to_be_bytes()).collect();
        v.resize(len, 0);
        v
    }

    #[test]
    fn parses_valid_v17_header() {
        let h = FdtHeader::parse(&blob(OK, 256)).unwrap();
        assert_eq!((h.version, h.totalsize, h.size_dt_struct), (17, 256, 144));
    }

    #[test]
    fn rejects_short_blob() {
        assert_eq!(FdtHeader::parse(&[0u8; 39]), Err(FdtError::Truncated));
    }

    #[test]
    fn rejects_bad_magic() {
        let mut f = OK;
        f[0] = 0x1234_5678;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::BadMagic(0x1234_5678)));
    }

    #[test]
    fn rejects_old_version() {
        let mut f = OK;
        f[5] = 16;
        f[6] = 16;
        assert_eq!(
            FdtHeader::parse(&blob(f, 256)),
            Err(FdtError::BadVersion { version: 16, last_comp_version: 16 })
        );
    }

    #[test]
    fn rejects_totalsize_beyond_blob() {
        let mut f = OK;
        f[1] = 512;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated));
    }

    #[test]
    fn rejects_totalsize_over_2mib() {
        let mut f = OK;
        f[1] = 2 * 1024 * 1024 + 8;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::TooLarge(2 * 1024 * 1024 + 8)));
    }

    #[test]
    fn rejects_misaligned_rsvmap() {
        let mut f = OK;
        f[4] = 44;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Alignment));
    }

    #[test]
    fn rejects_struct_block_past_end() {
        let mut f = OK;
        f[9] = 1000;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated));
    }

    #[test]
    fn rejects_last_comp_version_newer_than_17() {
        let mut f = OK;
        f[5] = 18;
        f[6] = 18;
        assert_eq!(
            FdtHeader::parse(&blob(f, 256)),
            Err(FdtError::BadVersion { version: 18, last_comp_version: 18 })
        );
    }

    #[test]
    fn accepts_newer_version_that_stays_compatible_with_16() {
        let mut f = OK;
        f[5] = 18;
        f[6] = 16;
        assert_eq!(FdtHeader::parse(&blob(f, 256)).map(|h| h.version), Ok(18));
    }

    #[test]
    fn rejects_misaligned_struct_block() {
        let mut f = OK;
        f[2] = 58;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Alignment));
    }

    #[test]
    fn rejects_rsvmap_past_end() {
        let mut f = OK;
        f[4] = 264;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated));
    }

    #[test]
    fn rejects_struct_block_overlapping_header() {
        let mut f = OK;
        f[2] = 0;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated));
    }

    #[test]
    fn rejects_overflowing_strings_block_without_panicking() {
        let mut f = OK;
        f[3] = 0xFFFF_FFF0;
        f[8] = 0x20;
        assert_eq!(FdtHeader::parse(&blob(f, 256)), Err(FdtError::Truncated));
    }
}
