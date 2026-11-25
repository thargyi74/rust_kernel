// SPDX-License-Identifier: GPL-2.0-only
//! Walking a flattened device tree: nodes, properties and `reg`.
//!
//! The counterpart of libfdt's `scripts/dtc/libfdt/fdt_ro.c` and of the
//! early scans in `drivers/of/fdt.c`.
//!
//! [`Fdt::new`] checks the whole structure block once, like libfdt's
//! `fdt_check_full()` (`scripts/dtc/libfdt/libfdt.h:382`). After that, every
//! [`Node`] method walks bytes already known to be good. They still use
//! `.get()` everywhere, so even a bug here can't panic on a bad index.

use super::{FdtError, FdtHeader};

/// Structure-block tokens (`scripts/dtc/libfdt/fdt.h:53-58`).
const FDT_BEGIN_NODE: u32 = 0x1;
const FDT_END_NODE: u32 = 0x2;
const FDT_PROP: u32 = 0x3;
const FDT_NOP: u32 = 0x4;
const FDT_END: u32 = 0x9;

/// Deepest nesting we accept; the root node is depth 1.
const MAX_DEPTH: usize = 32;

/// Cells used when no `#address-cells` or `#size-cells` is found
/// (`OF_ROOT_NODE_{ADDR,SIZE}_CELLS_DEFAULT`, `drivers/of/of_private.h:36,39`).
const DEFAULT_CELLS: Cells = Cells { address: 1, size: 1 };

/// Largest `#address-cells` or `#size-cells` whose values fit in a `u64`.
const MAX_CELLS: u32 = 2;

/// A device tree whose header and structure block have been checked.
pub struct Fdt<'a> {
    blob: &'a [u8],
    header: FdtHeader,
    structs: &'a [u8],
    strings: &'a [u8],
}

/// One node of a checked device tree.
#[derive(Clone, Copy)]
pub struct Node<'a> {
    structs: &'a [u8],
    strings: &'a [u8],
    name: &'a str,
    /// Offset of the first token after the name: a property, a child or
    /// the node's `END_NODE`.
    body: usize,
    /// The `#address-cells` and `#size-cells` that apply to this node's `reg`.
    cells: Cells,
}

#[derive(Clone, Copy)]
struct Cells {
    address: u32,
    size: u32,
}

/// One structure-block token.
enum Token<'a> {
    BeginNode { name: &'a str },
    EndNode,
    Prop { name: &'a str, value: &'a [u8] },
    Nop,
    End,
}

impl<'a> Fdt<'a> {
    /// Checks the header and the whole structure block of `blob`.
    pub fn new(blob: &'a [u8]) -> Result<Fdt<'a>, FdtError> {
        let header = FdtHeader::parse(blob)?;
        let block = |off: u32, size: u32| {
            let start = off as usize;
            blob.get(start..start.checked_add(size as usize)?)
        };
        let structs = block(header.off_dt_struct, header.size_dt_struct).ok_or(FdtError::Truncated)?;
        let strings = block(header.off_dt_strings, header.size_dt_strings).ok_or(FdtError::Truncated)?;
        check_structure(structs, strings)?;
        Ok(Fdt { blob, header, structs, strings })
    }

    pub fn header(&self) -> &FdtHeader {
        &self.header
    }

    /// The root node. [`Fdt::new`] made sure it exists and has an empty name.
    pub fn root(&self) -> Node<'a> {
        let mut off = 0;
        while let Ok((Token::Nop, next)) = read_token(self.structs, self.strings, off) {
            off = next;
        }
        let body = match read_token(self.structs, self.strings, off) {
            Ok((Token::BeginNode { .. }, next)) => next,
            // Unreachable after the check; an empty node is the safe answer.
            _ => self.structs.len(),
        };
        Node { structs: self.structs, strings: self.strings, name: "", body, cells: DEFAULT_CELLS }
    }

    /// Looks up an absolute path such as `/soc/uart@1000`. A component
    /// without `@` also matches a node named `component@…`.
    pub fn find_node(&self, path: &str) -> Option<Node<'a>> {
        let rest = path.strip_prefix('/')?;
        let mut node = self.root();
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            node = node.children().find(|c| name_matches(c.name(), part))?;
        }
        Some(node)
    }

    /// The node named by `/chosen` `stdout-path`, without any `:options`.
    /// A path that doesn't start with `/` is an alias from `/aliases`.
    pub fn chosen_stdout(&self) -> Option<Node<'a>> {
        let stdout = self.find_node("/chosen")?.property_str("stdout-path")?;
        let path = stdout.split(':').next()?;
        if path.starts_with('/') {
            self.find_node(path)
        } else {
            self.find_node(self.find_node("/aliases")?.property_str(path)?)
        }
    }

    /// Children of the root whose `device_type` is `"memory"` and that are
    /// not disabled, as in `early_init_dt_scan_memory()`
    /// (`drivers/of/fdt.c:1050-1054`).
    pub fn memory_nodes(&self) -> impl Iterator<Item = Node<'a>> + use<'a> {
        self.root().children().filter(|n| {
            n.property_str("device_type") == Some("memory")
                && matches!(n.property_str("status"), None | Some("okay") | Some("ok"))
        })
    }

    /// The memory-reservation map as `(address, size)` pairs. It stops at
    /// the first entry with size 0, as libfdt's `fdt_num_mem_rsv()` does, or
    /// at the end of the blob if that entry is missing.
    pub fn mem_reservations(&self) -> impl Iterator<Item = (u64, u64)> + use<'a> {
        let total = self.header.totalsize as usize;
        let map = self.blob.get(self.header.off_mem_rsvmap as usize..total).unwrap_or(&[]);
        let (entries, _) = map.as_chunks::<16>();
        entries
            .iter()
            .map(|e| (be64(e, 0).unwrap_or(0), be64(e, 8).unwrap_or(0)))
            .take_while(|&(_, size)| size != 0)
    }
}

impl<'a> Node<'a> {
    /// The node's name, including any `@unit-address`. The root's is `""`.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The raw value of property `name`.
    pub fn property(&self, name: &str) -> Option<&'a [u8]> {
        self.properties().find(|&(n, _)| n == name).map(|(_, v)| v)
    }

    /// The first string of property `name`, as `of_property_read_string()`
    /// returns it. `None` if the value has no NUL or isn't UTF-8.
    pub fn property_str(&self, name: &str) -> Option<&'a str> {
        let value = self.property(name)?;
        let nul = value.iter().position(|&b| b == 0)?;
        core::str::from_utf8(value.get(..nul)?).ok()
    }

    /// Whether `c` is one of the strings in `compatible`.
    pub fn is_compatible(&self, c: &str) -> bool {
        match self.property("compatible") {
            Some(list) => list.split(|&b| b == 0).any(|s| s == c.as_bytes()),
            None => false,
        }
    }

    /// The direct children, in order.
    pub fn children(&self) -> impl Iterator<Item = Node<'a>> + use<'a> {
        let (structs, strings) = (self.structs, self.strings);
        let cells = self.child_cells();
        let mut off = self.body;
        core::iter::from_fn(move || {
            loop {
                let (token, next) = read_token(structs, strings, off).ok()?;
                match token {
                    Token::Nop | Token::Prop { .. } => off = next,
                    Token::BeginNode { name } => {
                        off = skip_subtree(structs, strings, next)?;
                        return Some(Node { structs, strings, name, body: next, cells });
                    }
                    Token::EndNode | Token::End => return None,
                }
            }
        })
    }

    /// The `(address, size)` pairs of `reg`, decoded with the parent's
    /// cells. A missing `reg` gives an empty iterator.
    pub fn reg(&self) -> Result<impl Iterator<Item = (u64, u64)> + use<'a>, FdtError> {
        let Cells { address, size } = self.cells;
        if address > MAX_CELLS || size > MAX_CELLS {
            return Err(FdtError::UnsupportedCells { address, size });
        }
        let value = self.property("reg").unwrap_or(&[]);
        let address_len = address as usize * 4;
        let entry_len = address_len + size as usize * 4;
        let whole = if entry_len == 0 { value.is_empty() } else { value.len().is_multiple_of(entry_len) };
        if !whole {
            return Err(FdtError::BadReg);
        }
        Ok(value.chunks_exact(entry_len.max(1)).map(move |entry| {
            let (a, s) = entry.split_at_checked(address_len).unwrap_or((entry, &[]));
            (cells_value(a), cells_value(s))
        }))
    }

    /// The properties, in order.
    fn properties(&self) -> impl Iterator<Item = (&'a str, &'a [u8])> + use<'a> {
        let (structs, strings) = (self.structs, self.strings);
        let mut off = self.body;
        core::iter::from_fn(move || {
            loop {
                let (token, next) = read_token(structs, strings, off).ok()?;
                off = next;
                match token {
                    Token::Nop => {}
                    Token::Prop { name, value } => return Some((name, value)),
                    _ => return None,
                }
            }
        })
    }

    /// The cells this node passes to its children: its own
    /// `#address-cells`/`#size-cells`, or else the ones it inherited.
    fn child_cells(&self) -> Cells {
        let own = |name| self.property(name).and_then(|v| be32(v, 0).filter(|_| v.len() == 4));
        Cells {
            address: own("#address-cells").unwrap_or(self.cells.address),
            size: own("#size-cells").unwrap_or(self.cells.size),
        }
    }
}

/// `part` matches `name` exactly, or, if `part` has no `@`, matches the
/// part of `name` before its `@`.
fn name_matches(name: &str, part: &str) -> bool {
    name == part || (!part.contains('@') && name.split('@').next() == Some(part))
}

/// The whole-block check of `fdt_check_full()`: every token is known and in
/// bounds, names and strings are terminated UTF-8, nodes nest properly and
/// no deeper than [`MAX_DEPTH`], and `FDT_END` follows the closed root.
fn check_structure(structs: &[u8], strings: &[u8]) -> Result<(), FdtError> {
    let mut off = 0;
    let mut depth = 0;
    let mut root_seen = false;
    loop {
        let bad = FdtError::BadStructure { offset: off };
        let (token, next) = read_token(structs, strings, off)?;
        match token {
            Token::Nop => {}
            Token::BeginNode { name } => {
                if depth == 0 && (root_seen || !name.is_empty()) {
                    return Err(bad);
                }
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(bad);
                }
                root_seen = true;
            }
            Token::EndNode => {
                if depth == 0 {
                    return Err(bad);
                }
                depth -= 1;
            }
            Token::Prop { .. } => {
                if depth == 0 {
                    return Err(bad);
                }
            }
            Token::End => {
                return if depth == 0 && root_seen { Ok(()) } else { Err(bad) };
            }
        }
        off = next;
    }
}

/// Reads the token at `off` and returns it with the offset of the next one.
/// Anything malformed is `BadStructure { offset: off }`.
fn read_token<'a>(structs: &'a [u8], strings: &'a [u8], off: usize) -> Result<(Token<'a>, usize), FdtError> {
    let bad = FdtError::BadStructure { offset: off };
    let after_tag = off.checked_add(4).ok_or(bad)?;
    match be32(structs, off).ok_or(bad)? {
        FDT_BEGIN_NODE => {
            let name = c_str(structs.get(after_tag..).ok_or(bad)?).ok_or(bad)?;
            Ok((Token::BeginNode { name }, align4(after_tag + name.len() + 1).ok_or(bad)?))
        }
        FDT_END_NODE => Ok((Token::EndNode, after_tag)),
        FDT_PROP => {
            let len = be32(structs, after_tag).ok_or(bad)? as usize;
            let nameoff = be32(structs, after_tag + 4).ok_or(bad)? as usize;
            let value_start = after_tag + 8;
            let value_end = value_start.checked_add(len).ok_or(bad)?;
            let value = structs.get(value_start..value_end).ok_or(bad)?;
            let name = c_str(strings.get(nameoff..).ok_or(bad)?).ok_or(bad)?;
            Ok((Token::Prop { name, value }, align4(value_end).ok_or(bad)?))
        }
        FDT_NOP => Ok((Token::Nop, after_tag)),
        FDT_END => Ok((Token::End, after_tag)),
        _ => Err(bad),
    }
}

/// From just after a child's name to just after its `END_NODE`.
fn skip_subtree(structs: &[u8], strings: &[u8], mut off: usize) -> Option<usize> {
    let mut depth = 1usize;
    loop {
        let (token, next) = read_token(structs, strings, off).ok()?;
        off = next;
        match token {
            Token::BeginNode { .. } => depth += 1,
            Token::EndNode => {
                depth -= 1;
                if depth == 0 {
                    return Some(off);
                }
            }
            Token::End => return None,
            Token::Nop | Token::Prop { .. } => {}
        }
    }
}

/// The NUL-terminated UTF-8 string at the start of `bytes`.
fn c_str(bytes: &[u8]) -> Option<&str> {
    let nul = bytes.iter().position(|&b| b == 0)?;
    core::str::from_utf8(bytes.get(..nul)?).ok()
}

fn align4(off: usize) -> Option<usize> {
    Some(off.checked_add(3)? & !3)
}

fn be32(bytes: &[u8], off: usize) -> Option<u32> {
    let b = bytes.get(off..off.checked_add(4)?)?;
    Some(u32::from_be_bytes(b.try_into().ok()?))
}

fn be64(bytes: &[u8], off: usize) -> Option<u64> {
    Some((u64::from(be32(bytes, off)?) << 32) | u64::from(be32(bytes, off.checked_add(4)?)?))
}

/// Big-endian cells joined into one number; at most 2 cells reach here.
fn cells_value(bytes: &[u8]) -> u64 {
    let (cells, _) = bytes.as_chunks::<4>();
    cells.iter().fold(0, |acc, &c| (acc << 32) | u64::from(u32::from_be_bytes(c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fdt::testing::Builder;
    use crate::fdt::FdtError;

    const QEMU: &[u8] = include_bytes!("../../testdata/qemu-virt-512m.dtb");

    #[test]
    fn qemu_memory_node_is_512_mib_at_1_gib() {
        let fdt = Fdt::new(QEMU).unwrap();
        let mem: Vec<_> = fdt.memory_nodes().flat_map(|n| n.reg().unwrap()).collect();
        assert_eq!(mem, vec![(0x4000_0000, 0x2000_0000)]);
    }

    #[test]
    fn qemu_stdout_is_the_pl011() {
        let uart = Fdt::new(QEMU).unwrap().chosen_stdout().unwrap();
        assert_eq!(uart.name(), "pl011@9000000");
        assert!(uart.is_compatible("arm,pl011"));
        assert_eq!(uart.reg().unwrap().next(), Some((0x0900_0000, 0x1000)));
    }

    #[test]
    fn qemu_psci_method_is_hvc() {
        assert_eq!(Fdt::new(QEMU).unwrap().find_node("/psci").unwrap().property_str("method"), Some("hvc"));
    }

    #[test]
    fn qemu_has_no_memreserve_entries() {
        assert_eq!(Fdt::new(QEMU).unwrap().mem_reservations().count(), 0);
    }

    #[test]
    fn find_node_matches_name_without_unit_address() {
        assert_eq!(Fdt::new(QEMU).unwrap().find_node("/memory").unwrap().name(), "memory@40000000");
    }

    /// A root node with 1/1 cells, still open: each hostile test adds one
    /// defect, then closes it.
    fn root() -> Builder {
        let mut b = Builder::new();
        b.begin("").prop_u32("#address-cells", &[1]).prop_u32("#size-cells", &[1]);
        b
    }

    fn is_bad_structure(blob: &[u8]) -> bool {
        matches!(Fdt::new(blob), Err(FdtError::BadStructure { .. }))
    }

    #[test]
    fn base_blob_without_defects_is_accepted() {
        let mut b = root();
        b.begin("child").prop_str("x", "y").end().end();
        assert!(Fdt::new(&b.finish()).is_ok());
    }

    #[test]
    fn stdout_path_alias_with_options() {
        let mut b = root();
        b.begin("aliases").prop_str("serial0", "/uart@1000").end();
        b.begin("chosen").prop_str("stdout-path", "serial0:115200n8").end();
        b.begin("uart@1000").prop_str("compatible", "arm,pl011").prop_u32("reg", &[0x1000, 0x100]).end();
        b.end();
        let blob = b.finish();
        let uart = Fdt::new(&blob).unwrap().chosen_stdout().unwrap();
        assert_eq!(uart.name(), "uart@1000");
        assert_eq!(uart.reg().unwrap().collect::<Vec<_>>(), vec![(0x1000, 0x100)]);
    }

    #[test]
    fn reg_uses_parent_cells() {
        let mut b = Builder::new();
        b.begin("").prop_u32("#address-cells", &[2]).prop_u32("#size-cells", &[2]);
        b.begin("soc").prop_u32("#address-cells", &[1]).prop_u32("#size-cells", &[1]);
        b.begin("dev@10").prop_u32("reg", &[0x10, 0x20]).end();
        b.end().end();
        let blob = b.finish();
        let dev = Fdt::new(&blob).unwrap().find_node("/soc/dev@10").unwrap();
        assert_eq!(dev.reg().unwrap().collect::<Vec<_>>(), vec![(0x10, 0x20)]);
    }

    #[test]
    fn disabled_memory_node_is_skipped() {
        let mut b = root();
        b.begin("memory@0")
            .prop_str("device_type", "memory")
            .prop_str("status", "disabled")
            .prop_u32("reg", &[0, 0x1000])
            .end();
        b.end();
        assert_eq!(Fdt::new(&b.finish()).unwrap().memory_nodes().count(), 0);
    }

    #[test]
    fn lists_memreserve_entries() {
        let mut b = root();
        b.reserve(0x4800_0000, 0x1000).end();
        let blob = b.finish();
        let rsv: Vec<_> = Fdt::new(&blob).unwrap().mem_reservations().collect();
        assert_eq!(rsv, vec![(0x4800_0000, 0x1000)]);
    }

    #[test]
    fn rejects_property_length_past_end() {
        let mut b = root();
        b.raw(3).raw(0x1000).raw(0).end();
        assert!(is_bad_structure(&b.finish()));
    }

    #[test]
    fn rejects_unterminated_node_name() {
        // The name's bytes run to the end of the struct block; the strings
        // block right after it does hold NULs, which must not count.
        let mut b = root();
        b.raw(1).raw(0x4141_4141);
        assert!(is_bad_structure(&b.finish_without_end()));
    }

    #[test]
    fn rejects_unknown_token() {
        let mut b = root();
        b.raw(7).end();
        assert!(is_bad_structure(&b.finish()));
    }

    #[test]
    fn rejects_missing_end_token() {
        let mut b = root();
        b.end();
        assert!(is_bad_structure(&b.finish_without_end()));
    }

    #[test]
    fn rejects_extra_end_node() {
        let mut b = root();
        b.end().end();
        assert!(is_bad_structure(&b.finish()));
    }

    #[test]
    fn rejects_string_offset_past_strings_block() {
        let mut b = root();
        b.raw(3).raw(0).raw(0xffff).end();
        assert!(is_bad_structure(&b.finish()));
    }

    /// The root plus `children` nested nodes below it.
    fn nested(children: usize) -> Vec<u8> {
        let mut b = root();
        for _ in 0..children {
            b.begin("n");
        }
        for _ in 0..children {
            b.end();
        }
        b.end();
        b.finish()
    }

    #[test]
    fn accepts_nesting_of_exactly_32() {
        assert!(Fdt::new(&nested(31)).is_ok());
    }

    #[test]
    fn rejects_nesting_deeper_than_32() {
        assert!(is_bad_structure(&nested(32)));
    }

    #[test]
    fn reg_with_three_address_cells_is_an_error() {
        let mut b = Builder::new();
        b.begin("").prop_u32("#address-cells", &[3]).prop_u32("#size-cells", &[1]);
        b.begin("dev").prop_u32("reg", &[0, 0, 0x10, 0x20]).end();
        b.end();
        let blob = b.finish();
        let dev = Fdt::new(&blob).unwrap().find_node("/dev").unwrap();
        assert_eq!(dev.reg().err(), Some(FdtError::UnsupportedCells { address: 3, size: 1 }));
    }

    #[test]
    fn reg_with_partial_entry_is_an_error() {
        let mut b = root();
        b.begin("dev").prop_u32("reg", &[0x10, 0x20, 0x30]).end();
        b.end();
        let blob = b.finish();
        let dev = Fdt::new(&blob).unwrap().find_node("/dev").unwrap();
        assert_eq!(dev.reg().err(), Some(FdtError::BadReg));
    }
}
