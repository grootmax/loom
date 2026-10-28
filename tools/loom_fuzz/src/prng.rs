//! Pseudo-random number generator for deterministic fuzzing.

/// A deterministic random number generator using `SplitMix64`.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a new PRNG with the given seed.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Generates the next random `u64`.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Generates a random `usize` in `[0, bound)`. Returns 0 if bound is 0.
    pub fn gen_usize(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        usize::try_from(self.next_u64() % (bound as u64)).unwrap_or(0)
    }

    /// Generates a random `usize` in `[min, max]`.
    pub fn gen_range(&mut self, min: usize, max: usize) -> usize {
        if min >= max {
            return min;
        }
        min + self.gen_usize(max - min + 1)
    }

    /// Returns `true` with probability `p` (in `0.0..=1.0`).
    pub fn gen_bool(&mut self, p: f64) -> bool {
        let clamped = p.clamp(0.0, 1.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "clamped range 0..1"
        )]
        let threshold = (clamped * (u64::MAX as f64)) as u64;
        self.next_u64() <= threshold
    }

    /// Selects a random element from a slice.
    ///
    /// # Panics
    ///
    /// Panics if `slice` is empty.
    pub fn choose<'a, T>(&mut self, slice: &'a [T]) -> &'a T {
        assert!(!slice.is_empty(), "cannot choose from an empty slice");
        let idx = self.gen_usize(slice.len());
        &slice[idx]
    }

    /// Generates a random byte.
    pub fn gen_byte(&mut self) -> u8 {
        #[allow(clippy::cast_possible_truncation, reason = "taking lowest byte")]
        {
            self.next_u64() as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determinism() {
        let mut rng1 = Rng::new(12345);
        let mut rng2 = Rng::new(12345);

        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }

    #[test]
    fn test_gen_range() {
        let mut rng = Rng::new(42);
        for _ in 0..1000 {
            let val = rng.gen_range(5, 15);
            assert!((5..=15).contains(&val));
        }
    }
}
