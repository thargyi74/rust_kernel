// SPDX-License-Identifier: GPL-2.0-only
//! Test-only builder for device-tree blobs.
//!
//! A small cousin of libfdt's sequential-write API in
//! `scripts/dtc/libfdt/fdt_sw.c`. It writes whatever it is told, including
//! broken structure, so the hostile tests can make one defect at a time.

use super::{FDT_MAGIC, FDT_V17_HEADER_SIZE};

const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const END: u32 = 9;

pub(crate) struct Builder {
    reservations: Vec<(u64, u64)>,
    structs: Vec<u8>,
    strings: Vec<u8>,
}

impl Builder {
    pub(crate) fn new() -> Builder {
        Builder { reservations: Vec::new(), structs: Vec::new(), strings: Vec::new() }
    }

    /// Adds a memory-reservation entry.
    pub(crate) fn reserve(&mut self, base: u64, size: u64) -> &mut Builder {
        self.reservations.push((base, size));
        self
    }

    /// Opens a node: `BEGIN_NODE`, then the NUL-terminated name, padded.
    pub(crate) fn begin(&mut self, name: &str) -> &mut Builder {
        self.raw(BEGIN_NODE);
        self.structs.extend_from_slice(name.as_bytes());
        self.structs.push(0);
        self.pad();
        self
    }

    /// Closes the open node.
    pub(crate) fn end(&mut self) -> &mut Builder {
        self.raw(END_NODE)
    }

    /// A property with a raw value.
    pub(crate) fn prop(&mut self, name: &str, value: &[u8]) -> &mut Builder {
        let nameoff = self.string(name);
        self.raw(PROP).raw(value.len() as u32).raw(nameoff);
        self.structs.extend_from_slice(value);
        self.pad();
        self
    }

    /// A property made of big-endian `u32` cells.
    pub(crate) fn prop_u32(&mut self, name: &str, cells: &[u32]) -> &mut Builder {
        let value: Vec<u8> = cells.iter().flat_map(|c| c.to_be_bytes()).collect();
        self.prop(name, &value)
    }

    /// A property holding one NUL-terminated string.
    pub(crate) fn prop_str(&mut self, name: &str, s: &str) -> &mut Builder {
        let mut value = s.as_bytes().to_vec();
        value.push(0);
        self.prop(name, &value)
    }

    /// Writes any big-endian word into the structure block.
    pub(crate) fn raw(&mut self, word: u32) -> &mut Builder {
        self.structs.extend_from_slice(&word.to_be_bytes());
        self
    }

    /// The whole blob, with `FDT_END` closing the structure block.
    pub(crate) fn finish(&self) -> Vec<u8> {
        self.build(true)
    }

    /// The whole blob, without `FDT_END`.
    pub(crate) fn finish_without_end(&self) -> Vec<u8> {
        self.build(false)
    }

    /// The offset of `name` in the strings block, adding it if it is new.
    fn string(&mut self, name: &str) -> u32 {
        let mut off = 0;
        for s in self.strings.split(|&b| b == 0) {
            if s == name.as_bytes() {
                return off as u32;
            }
            off += s.len() + 1;
        }
        let off = self.strings.len();
        self.strings.extend_from_slice(name.as_bytes());
        self.strings.push(0);
        off as u32
    }

    fn pad(&mut self) {
        while !self.structs.len().is_multiple_of(4) {
            self.structs.push(0);
        }
    }

    /// Layout: header, reservation map, structure block, strings block.
    fn build(&self, with_end: bool) -> Vec<u8> {
        let mut structs = self.structs.clone();
        if with_end {
            structs.extend_from_slice(&END.to_be_bytes());
        }
        let off_mem_rsvmap = FDT_V17_HEADER_SIZE;
        let off_dt_struct = off_mem_rsvmap + (self.reservations.len() + 1) * 16;
        let off_dt_strings = off_dt_struct + structs.len();
        let totalsize = off_dt_strings + self.strings.len();

        let header = [
            FDT_MAGIC,
            totalsize as u32,
            off_dt_struct as u32,
            off_dt_strings as u32,
            off_mem_rsvmap as u32,
            17, // version
            16, // last_comp_version
            0,  // boot_cpuid_phys
            self.strings.len() as u32,
            structs.len() as u32,
        ];
        let mut blob: Vec<u8> = header.iter().flat_map(|w| w.to_be_bytes()).collect();
        for &(base, size) in self.reservations.iter().chain(&[(0, 0)]) {
            blob.extend_from_slice(&base.to_be_bytes());
            blob.extend_from_slice(&size.to_be_bytes());
        }
        blob.extend_from_slice(&structs);
        blob.extend_from_slice(&self.strings);
        blob
    }
}
