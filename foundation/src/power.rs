// SPDX-License-Identifier: GPL-2.0-only
//! Machine power control through PSCI firmware.
//! Linux counterpart: `kernel_power_off` (`kernel/reboot.c:715`), and the
//! conduit choice of `get_set_conduit_method`
//! (`drivers/firmware/psci/psci.c:287`).

use core::sync::atomic::{AtomicU8, Ordering::Relaxed};
use klib::fdt::Fdt;

/// How PSCI calls reach the firmware (Linux: `enum arm_smccc_conduit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conduit {
    Hvc,
    Smc,
}

/// Why [`init`] set no conduit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsciError {
    /// The device tree has no `/psci` node.
    NoPsciNode,
    /// `/psci` has no `method` property.
    MissingMethod,
    /// `method` is neither `"hvc"` nor `"smc"`.
    InvalidMethod,
}

const NO_CONDUIT: u8 = 0;
const HVC: u8 = 1;
const SMC: u8 = 2;

/// The conduit, as one of the constants above. Only plain loads and stores:
/// the panic path powers off with the MMU off.
static CONDUIT: AtomicU8 = AtomicU8::new(NO_CONDUIT);

/// Reads `/psci` `method` and remembers the conduit for [`power_off`].
pub fn init(fdt: &Fdt<'_>) -> Result<Conduit, PsciError> {
    let psci = fdt.find_node("/psci").ok_or(PsciError::NoPsciNode)?;
    psci.property("method").ok_or(PsciError::MissingMethod)?;
    let conduit = match psci.property_str("method") {
        Some("hvc") => Conduit::Hvc,
        Some("smc") => Conduit::Smc,
        _ => return Err(PsciError::InvalidMethod),
    };
    CONDUIT.store(if conduit == Conduit::Hvc { HVC } else { SMC }, Relaxed);
    Ok(conduit)
}

/// Powers the machine off. Never returns. With no conduit set, it halts the
/// CPU instead.
pub fn power_off() -> ! {
    match CONDUIT.load(Relaxed) {
        HVC => crate::arch::system_off(Conduit::Hvc),
        SMC => crate::arch::system_off(Conduit::Smc),
        _ => crate::arch::halt(),
    }
}
