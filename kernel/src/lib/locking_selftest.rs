// SPDX-License-Identifier: GPL-2.0-only
//! Linux counterpart: `lib/locking-selftest.c`, much smaller: one CPU, so
//! it checks the lock's own logic, not contention.

use crate::pr_info;
use foundation::sync::SpinLock;

/// Checks `SpinLock` once the MMU is on.
pub fn locking_selftest() {
    let lock = SpinLock::new(0u32);
    {
        let mut guard = lock.lock();
        assert!(lock.try_lock().is_none(), "spinlock self-test: try_lock took a held lock");
        *guard = 42;
    }
    let value = *lock.try_lock().expect("spinlock self-test: dropping the guard did not unlock");
    assert_eq!(value, 42, "spinlock self-test: value changed under the lock was lost");

    // More cycles than a u16 ticket holds, so both tickets wrap.
    const CYCLES: u32 = 70_000;
    let counter = SpinLock::new(0u32);
    for _ in 0..CYCLES {
        *counter.lock() += 1;
    }
    assert_eq!(*counter.lock(), CYCLES, "spinlock self-test: lost an update");
    assert!(counter.try_lock().is_some(), "spinlock self-test: try_lock failed after the wrap");
    pr_info!("rust_kernel: spinlock self-test OK");
}
