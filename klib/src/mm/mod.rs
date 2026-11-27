// SPDX-License-Identifier: GPL-2.0-only
//! Memory management algorithms. Linux counterpart: `mm/`.

pub mod addr;
pub mod buddy;
pub mod layout;
pub mod map;
pub mod memblock;
pub mod slab;
