// SPDX-License-Identifier: GPL-2.0-only
//! A ticket spinlock. Linux counterpart: `include/asm-generic/ticket_spinlock.h:33-69`
//! (arm64 uses a qspinlock; a ticket lock is the simpler fair lock, see the
//! M2 study note).
//!
//! Linux keeps both tickets in one `u32` and stores into its low `u16`
//! half. That mixed-size atomic access is not allowed in Rust's memory
//! model, so `next` and `owner` are two `AtomicU16`. Both wrap at
//! `u16::MAX`.
//!
//! No interrupt masking yet: there are no interrupts until M3, which adds
//! `lock_irqsave`.

use crate::mm::mmu_enabled;
use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::AtomicU16;
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};

/// A value that one CPU at a time may use, taken in ticket order.
pub struct SpinLock<T> {
    /// The ticket the next locker gets.
    next: AtomicU16,
    /// The ticket being served: the holder's, or `next` if the lock is free.
    owner: AtomicU16,
    value: UnsafeCell<T>,
}

// SAFETY: the lock gives `&mut T` to one guard at a time, so sharing the
// lock between CPUs only moves `T` between them, which `T: Send` allows.
// (`SpinLock<T>` is `Send` for `T: Send` automatically.)
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        SpinLock { next: AtomicU16::new(0), owner: AtomicU16::new(0), value: UnsafeCell::new(value) }
    }

    /// Takes a ticket and spins until it is served (Linux: `ticket_spin_lock`).
    ///
    /// # Panics
    ///
    /// If the MMU is off: its read-modify-write atomics are unreliable on
    /// the Device memory the CPU sees then (QEMU happens to run them).
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        assert!(mmu_enabled(), "SpinLock used before the MMU is on");
        let ticket = self.next.fetch_add(1, Relaxed);
        while self.owner.load(Acquire) != ticket {
            spin_loop();
        }
        SpinLockGuard { lock: self, _not_send: PhantomData }
    }

    /// Takes the lock only if it is free (Linux: `ticket_spin_trylock`).
    ///
    /// # Panics
    ///
    /// If the MMU is off, as [`lock`](Self::lock).
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        assert!(mmu_enabled(), "SpinLock used before the MMU is on");
        let owner = self.owner.load(Acquire);
        // Free means `next == owner`; taking ticket `owner` takes the lock.
        self.next.compare_exchange(owner, owner.wrapping_add(1), Acquire, Relaxed).ok()?;
        Some(SpinLockGuard { lock: self, _not_send: PhantomData })
    }
}

/// The held lock. It gives access to the value and unlocks when dropped.
/// It is `!Send`: the CPU that locked it unlocks it.
pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    _not_send: PhantomData<*const ()>,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: this guard holds the lock, so no other guard exists and
        // nobody else touches the value until it drops.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`; `&mut self` makes this the only borrow.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    /// Serves the next ticket (Linux: `ticket_spin_unlock`).
    fn drop(&mut self) {
        self.lock.owner.fetch_add(1, Release);
    }
}
