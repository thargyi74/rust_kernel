// SPDX-License-Identifier: GPL-2.0-only
//! Project rules that the compiler cannot check by itself.
//!
//! "No crates": every package in the workspace's `Cargo.lock` must be one of
//! our own path crates. Cargo writes a `source = ...` line only for packages
//! that come from a registry (crates.io) or git, so any such line breaks the
//! rule. This covers normal, build and dev dependencies of every crate,
//! including xtask, and cannot be fooled by a registry crate that happens to
//! share a name with one of ours.

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

#[cfg(test)]
mod tests {
    use super::*;

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
