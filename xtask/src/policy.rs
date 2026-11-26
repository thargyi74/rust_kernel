// SPDX-License-Identifier: GPL-2.0-only
//! Project rules that the compiler cannot check by itself.
//!
//! "No crates": every package in the workspace's `Cargo.lock` must be one of
//! our own path crates. Cargo writes a `source = ...` line only for packages
//! that come from a registry (crates.io) or git, so any such line breaks the
//! rule. This covers normal, build and dev dependencies of every crate,
//! including xtask, and cannot be fooled by a registry crate that happens to
//! share a name with one of ours.
//!
//! "MMU-off instructions": code that runs while the MMU is off (the boot
//! path, the console, the panic path) must not use atomic read-modify-write
//! or `dc zva`. On real hardware they are unreliable or fault on the Device
//! memory the CPU sees then; QEMU runs them fine, so no boot scenario can
//! catch them. [`functions_using`] finds them in the kernel's disassembly.

use std::collections::BTreeSet;

/// Names of the `[[package]]` entries in `cargo_lock` that have a `source`.
pub fn outside_packages(cargo_lock: &str) -> Vec<String> {
    let mut outside = Vec::new();
    let mut name: Option<&str> = None;
    for line in cargo_lock.lines().map(str::trim) {
        if line == "[[package]]" {
            name = None;
        } else if let Some(n) = line.strip_prefix("name = ") {
            name = Some(n.trim_matches('"'));
        } else if line.starts_with("source = ") {
            outside.push(name.unwrap_or("<unnamed package>").to_string());
        }
    }
    outside
}

/// The only functions that may hold atomic read-modify-write instructions:
/// the two that `SpinLock` uses, which asserts the MMU is on first. A new
/// entry needs a reason why nothing reaches it with the MMU off.
pub const ALLOWED_RMW: &[&str] = &[
    "<core::sync::atomic::Atomic<u16>>::fetch_add",
    "core::sync::atomic::atomic_compare_exchange::<u16>",
];

/// Instructions the MMU-off rule forbids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forbidden {
    /// An atomic read-modify-write: the exclusives (`ldxr`/`stxr` and
    /// friends) or an LSE atomic (`cas`, `swp`, `ldadd`, ...).
    Rmw,
    /// `dc zva`, which faults on Device memory.
    DcZva,
}

/// Which forbidden class, if any, an AArch64 instruction belongs to.
pub fn classify(mnemonic: &str, operands: &str) -> Option<Forbidden> {
    let size = |s: &str| matches!(s, "" | "b" | "h");
    let order_size = |s: &str| {
        let s = s.strip_prefix('a').unwrap_or(s);
        size(s.strip_prefix('l').unwrap_or(s))
    };
    let release_size = |s: &str| size(s.strip_prefix('l').unwrap_or(s));
    let any = |bases: &[&str], suffix_ok: &dyn Fn(&str) -> bool| {
        bases.iter().any(|b| mnemonic.strip_prefix(b).is_some_and(suffix_ok))
    };
    let exclusive = any(&["ldxr", "ldaxr", "stxr", "stlxr", "ldxp", "ldaxp", "stxp", "stlxp"], &size);
    let lse = any(
        &["cas", "casp", "swp", "ldadd", "ldclr", "ldeor", "ldset", "ldsmax", "ldsmin", "ldumax", "ldumin"],
        &order_size,
    );
    let lse_store = any(&["stadd", "stclr", "steor", "stset", "stsmax", "stsmin", "stumax", "stumin"], &release_size);
    if exclusive || lse || lse_store {
        Some(Forbidden::Rmw)
    } else if mnemonic == "dc" && operands.trim_start().starts_with("zva") {
        Some(Forbidden::DcZva)
    } else {
        None
    }
}

/// The functions in `disasm` that hold an instruction of `class`. `disasm`
/// is the output of `llvm-objdump -d --no-show-raw-insn -C`: a line
/// `<hex> <name>:` starts a function, and `<hex>:\t<mnemonic>\t<operands>`
/// is an instruction.
pub fn functions_using(disasm: &str, class: Forbidden) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut function: Option<&str> = None;
    for line in disasm.lines() {
        let Some((addr, rest)) = line.split_once(|c: char| c == ' ' || c == ':') else { continue };
        if addr.is_empty() || !addr.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if let Some(name) = rest.strip_prefix('<').and_then(|r| r.strip_suffix(">:")) {
            function = Some(name);
            continue;
        }
        let mut fields = line.split('\t').skip(1);
        let mnemonic = fields.next().unwrap_or("").trim();
        let operands = fields.next().unwrap_or("");
        if classify(mnemonic, operands) == Some(class) {
            found.insert(function.unwrap_or("<no function>").to_string());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISASM: &str = "\
vmlinux:\tfile format elf64-littleaarch64

Disassembly of section .text:

0000000040081738 <<core::sync::atomic::Atomic<u16>>::fetch_add>:
40081738:      \tldaxrh\tw8, [x0]
4008173c:      \tadd\tw9, w8, w1
40081740:      \tstlxrh\tw10, w9, [x0]

0000000040081800 <kernel::kernel::panic::panic>:
40081800:      \tldarb\tw8, [x0]
40081804:      \tstlrb\tw9, [x0]
40081808:      \tldr\tx0, [x1]

0000000040081900 <lse::user>:
40081900:      \tcasal\tw0, w1, [x2]
40081904:      \tswpal\tw0, w1, [x2]

0000000040081a00 <memset>:
40081a00:      \tdc\tzva, x0
40081a04:      \tdc\tcivac, x0
";

    #[test]
    fn finds_exclusives_and_lse_atomics_by_function() {
        let found = functions_using(DISASM, Forbidden::Rmw);
        let want: Vec<&str> = vec!["<core::sync::atomic::Atomic<u16>>::fetch_add", "lse::user"];
        assert_eq!(found.iter().map(String::as_str).collect::<Vec<_>>(), want);
    }

    #[test]
    fn acquire_loads_and_release_stores_are_not_read_modify_write() {
        assert!(!functions_using(DISASM, Forbidden::Rmw).contains("kernel::kernel::panic::panic"));
    }

    #[test]
    fn finds_dc_zva_but_not_other_cache_maintenance() {
        let found = functions_using(DISASM, Forbidden::DcZva);
        assert_eq!(found.into_iter().collect::<Vec<_>>(), vec!["memset".to_string()]);
    }

    #[test]
    fn every_read_modify_write_mnemonic_is_caught() {
        for m in ["ldxr", "ldaxrb", "stxrh", "stlxr", "ldaxp", "stlxp", "cash", "casp", "swpb", "ldaddal", "ldclrl",
            "ldeora", "ldset", "ldsmaxh", "lduminb", "stadd", "stclrl", "steor", "stset", "stumax"]
        {
            assert_eq!(classify(m, "w0, w1, [x2]"), Some(Forbidden::Rmw), "{m}");
        }
        for m in ["ldar", "stlr", "ldr", "str", "ldapr", "stp", "ldp", "casx"] {
            assert_eq!(classify(m, "w0, [x2]"), None, "{m}");
        }
    }

    #[test]
    fn workspace_only_lock_is_clean() {
        let lock = "version = 4\n\n[[package]]\nname = \"foundation\"\nversion = \"0.1.0\"\ndependencies = [\n \"klib\",\n]\n\n[[package]]\nname = \"klib\"\nversion = \"0.1.0\"\n";
        assert!(outside_packages(lock).is_empty());
    }

    #[test]
    fn flags_registry_package() {
        let lock = "[[package]]\nname = \"kernel\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"spin\"\nversion = \"0.9.8\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"6980e8d7511241f8acf4aebddbb1ff938df5eebe98691418c4468d0b72a96a67\"\n";
        assert_eq!(outside_packages(lock), vec!["spin".to_string()]);
    }

    #[test]
    fn flags_git_package_even_if_named_like_ours() {
        let lock = "[[package]]\nname = \"klib\"\nversion = \"0.1.0\"\nsource = \"git+https://example.com/klib#abc\"\n";
        assert_eq!(outside_packages(lock), vec!["klib".to_string()]);
    }
}
