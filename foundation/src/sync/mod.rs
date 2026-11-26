// SPDX-License-Identifier: GPL-2.0-only
//! Locking. Linux counterpart: `include/linux/spinlock.h`.

mod spinlock;

pub use spinlock::{SpinLock, SpinLockGuard};
