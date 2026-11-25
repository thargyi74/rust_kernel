// SPDX-License-Identifier: GPL-2.0-only
//! A tiny pseudo-random generator for stress tests. Not for security.
//! Linux counterpart (the idea only): `lib/random32.c`.

/// Marsaglia's xorshift64 with the shifts 13, 7, 17. Same seed, same
/// sequence, on the Mac and in the kernel.
#[derive(Debug, Clone)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// A generator started from `seed`. The state must never be 0, so a
    /// seed of 0 becomes 1.
    pub const fn new(seed: u64) -> Self {
        XorShift64 { state: if seed == 0 { 1 } else { seed } }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// A number in `0..n` (slightly biased, which is fine for tests).
    /// `below(0)` is 0.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift_matches_reference() {
        let mut x = XorShift64::new(1);
        assert_eq!([x.next_u64(), x.next_u64(), x.next_u64()], [0x4082_2041, 0x1000_4106_0c01_1441, 0x9b1e_842f_6e86_2629]);
    }

    #[test]
    fn seed_zero_becomes_one() {
        assert_eq!(XorShift64::new(0).next_u64(), XorShift64::new(1).next_u64());
    }

    #[test]
    fn below_stays_below() {
        let mut x = XorShift64::new(7);
        assert!((0..1000).all(|_| x.below(10) < 10));
    }
}
