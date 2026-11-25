// SPDX-License-Identifier: GPL-2.0-only
//! Project rules that the compiler cannot check by itself.

/// Crates that may appear in the kernel's dependency tree: our own, and
/// nothing from crates.io (Global Constraint: "no crates").
pub const KERNEL_CRATES: &[&str] = &["vmlinux", "foundation", "kernel", "klib"];

/// Names of crates in `cargo tree --prefix none` output that are not in
/// `allowed`. Each line starts with the crate name ("spin v0.9.8 ...").
pub fn external_crates(cargo_tree_output: &str, allowed: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = cargo_tree_output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| !allowed.contains(name))
        .map(str::to_string)
        .collect();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_workspace_crates_is_clean() {
        let tree = "vmlinux v0.1.0 (/r/vmlinux)\nfoundation v0.1.0 (/r/foundation)\nkernel v0.1.0 (/r/kernel)\nklib v0.1.0 (/r/klib)\n";
        assert!(external_crates(tree, &["vmlinux", "foundation", "kernel", "klib"]).is_empty());
    }

    #[test]
    fn flags_crates_io_dependency() {
        let tree = "vmlinux v0.1.0 (/r/vmlinux)\nspin v0.9.8\n";
        assert_eq!(
            external_crates(tree, &["vmlinux"]),
            vec!["spin".to_string()]
        );
    }
}
