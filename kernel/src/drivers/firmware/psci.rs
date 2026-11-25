// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `drivers/firmware/psci/psci.c` (`psci_dt_init` and
//! `get_set_conduit_method`). The file is `firmware/psci.rs`, not
//! `psci/psci.rs`, because a module `psci::psci` trips clippy's
//! `module_inception`.

use crate::pr_info;
use foundation::power::{self, PsciError};
use klib::fdt::Fdt;

/// Picks the PSCI conduit from the device tree, so that power-off works.
/// On failure, power-off halts the CPU instead; boot goes on.
pub fn psci_dt_init(fdt: &Fdt<'_>) {
    // Messages as at psci.c:291 and :294, with pr_fmt "psci: " (:7).
    pr_info!("psci: probing for conduit method from DT.");
    match power::init(fdt) {
        Ok(_) => {}
        Err(PsciError::MissingMethod) => pr_info!("psci: missing \"method\" property"),
        Err(PsciError::InvalidMethod) => pr_info!("psci: invalid \"method\" property"),
        Err(PsciError::NoPsciNode) => pr_info!("psci: no /psci node, power off will halt"),
    }
}
