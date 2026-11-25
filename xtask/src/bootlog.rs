// SPDX-License-Identifier: GPL-2.0-only
//! Check a kernel's serial output against what a scenario expects.

/// What a boot log must and must not contain.
#[derive(Debug, Clone, Copy)]
pub struct Expect {
    /// Substrings that must all appear, in this order.
    pub must_contain: &'static [&'static str],
    /// Substrings that must not appear anywhere.
    pub must_not_contain: &'static [&'static str],
}

/// Returns `Ok(())` if `log` meets `expect`, otherwise an error naming the
/// first problem followed by the whole log. Serial output uses `\r\n`, so line
/// endings are normalised first.
pub fn check(log: &str, expect: &Expect) -> Result<(), String> {
    let log = log.replace("\r\n", "\n");
    let fail = |problem: String| Err(format!("{problem}\n--- log ---\n{log}"));

    let mut rest = log.as_str();
    for wanted in expect.must_contain {
        match rest.find(wanted) {
            Some(at) => rest = &rest[at + wanted.len()..],
            None => return fail(format!("missing (in order): {wanted:?}")),
        }
    }
    for forbidden in expect.must_not_contain {
        if log.contains(forbidden) {
            return fail(format!("forbidden: {forbidden:?}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: Expect = Expect {
        must_contain: &["A", "B"],
        must_not_contain: &["Kernel panic"],
    };

    #[test]
    fn accepts_log_with_all_lines() {
        assert!(check("x\r\nA\r\nB\r\n", &E).is_ok());
    }

    #[test]
    fn reports_missing_line() {
        assert!(
            check("A\n", &E)
                .unwrap_err()
                .contains("missing (in order): \"B\"")
        );
    }

    #[test]
    fn enforces_order() {
        assert!(
            check("B\nA\n", &E)
                .unwrap_err()
                .contains("missing (in order): \"B\"")
        );
    }

    #[test]
    fn rejects_forbidden_text() {
        let err = check("A\nB\nKernel panic - not syncing: x\n", &E).unwrap_err();
        assert!(err.contains("forbidden: \"Kernel panic\""));
    }
}
