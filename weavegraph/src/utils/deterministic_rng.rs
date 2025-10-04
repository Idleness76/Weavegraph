//! Deterministic random number generation for testing and reproducible behavior.
//!
//! Provides deterministic random number generators that can be seeded for
//! reproducible test scenarios and deterministic workflow execution when
//! randomness is required but consistency is needed.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;

/// Deterministic random number generator wrapping rand::StdRng.
///
/// This generator provides deterministic random values when seeded with a
/// fixed value, enabling reproducible test scenarios and consistent behavior
/// across runs when needed.
///
/// # Examples
///
/// ```rust
/// use weavegraph::utils::deterministic_rng::DeterministicRng;
///
/// let mut rng = DeterministicRng::new(42);
/// let value1 = rng.random_u64();
///
/// // Same seed produces same sequence
/// let mut rng2 = DeterministicRng::new(42);
/// let value2 = rng2.random_u64();
/// assert_eq!(value1, value2);
/// ```
#[derive(Debug)]
pub struct DeterministicRng {
    rng: StdRng,
    seed: u64,
}

impl DeterministicRng {
    /// Create a deterministic RNG with a fixed seed.
    ///
    /// # Parameters
    /// * `seed` - Seed value for deterministic generation
    ///
    /// # Returns
    /// A new DeterministicRng instance seeded with the given value.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            seed,
        }
    }

    /// Get the seed used to initialize this RNG.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Reset the RNG to its initial state using the original seed.
    pub fn reset(&mut self) {
        self.rng = StdRng::seed_from_u64(self.seed);
    }

    /// Create a new RNG with a different seed derived from the current state.
    #[must_use]
    pub fn fork(&mut self) -> Self {
        let new_seed = self.random_u64();
        Self::new(new_seed)
    }

    /// Generate a random u64 value.
    pub fn random_u64(&mut self) -> u64 {
        self.rng.random()
    }

    /// Generate a random u32 value.
    pub fn random_u32(&mut self) -> u32 {
        self.rng.random()
    }

    /// Generate a random boolean value.
    pub fn random_bool(&mut self) -> bool {
        self.rng.random()
    }

    /// Generate a random f64 value between 0.0 and 1.0.
    pub fn random_f64(&mut self) -> f64 {
        self.rng.random()
    }

    /// Generate a random u32 value in the specified range.
    ///
    /// # Parameters
    /// * `min` - Minimum value (inclusive)
    /// * `max` - Maximum value (exclusive)
    pub fn random_range_u32(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            return min;
        }
        min + (self.rng.random::<u32>() % (max - min))
    }

    /// Generate a random string of lowercase letters.
    ///
    /// # Parameters
    /// * `len` - Length of the string to generate
    ///
    /// # Examples
    ///
    /// ```rust
    /// use weavegraph::utils::deterministic_rng::DeterministicRng;
    ///
    /// let mut rng = DeterministicRng::new(42);
    /// let random_string = rng.random_string(8);
    /// assert_eq!(random_string.len(), 8);
    /// assert!(random_string.chars().all(|c| c.is_ascii_lowercase()));
    /// ```
    pub fn random_string(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| {
                let byte = self.random_range_u32(b'a' as u32, b'z' as u32 + 1) as u8;
                byte as char
            })
            .collect()
    }

    /// Generate a random alphanumeric string.
    ///
    /// # Parameters
    /// * `len` - Length of the string to generate
    ///
    /// # Examples
    /// ```rust
    /// use weavegraph::utils::deterministic_rng::DeterministicRng;
    ///
    /// let mut rng = DeterministicRng::new(42);
    /// let id = rng.random_alphanumeric(12);
    /// assert_eq!(id.len(), 12);
    /// assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    /// ```
    pub fn random_alphanumeric(&mut self, len: usize) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        (0..len)
            .map(|_| {
                let idx = self.random_range_u32(0, CHARS.len() as u32) as usize;
                CHARS[idx] as char
            })
            .collect()
    }

    /// Choose a random element from a slice.
    ///
    /// # Parameters
    /// * `choices` - Slice to choose from
    ///
    /// # Returns
    /// A reference to a randomly chosen element, or None if slice is empty.
    pub fn choose<'a, T>(&mut self, choices: &'a [T]) -> Option<&'a T> {
        if choices.is_empty() {
            None
        } else {
            let idx = self.random_range_u32(0, choices.len() as u32) as usize;
            Some(&choices[idx])
        }
    }
}

/// Thread-safe registry for managing multiple named deterministic RNG instances.
#[derive(Debug)]
pub struct RngRegistry {
    rngs: HashMap<String, DeterministicRng>,
    base_seed: u64,
}

impl RngRegistry {
    /// Create a new RNG registry with a base seed.
    #[must_use]
    pub fn new(base_seed: u64) -> Self {
        Self {
            rngs: HashMap::new(),
            base_seed,
        }
    }

    /// Get or create a deterministic RNG for a specific name.
    pub fn get_rng(&mut self, name: &str) -> &mut DeterministicRng {
        self.rngs.entry(name.to_string()).or_insert_with(|| {
            // Create deterministic seed from name and base seed
            let name_hash = name
                .bytes()
                .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
            let derived_seed = self.base_seed.wrapping_add(name_hash);
            DeterministicRng::new(derived_seed)
        })
    }

    /// Reset all RNGs in the registry to their initial state.
    pub fn reset_all(&mut self) {
        for rng in self.rngs.values_mut() {
            rng.reset();
        }
    }

    /// Get the number of RNGs in the registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rngs.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rngs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_behavior() {
        let mut rng1 = DeterministicRng::new(42);
        let mut rng2 = DeterministicRng::new(42);

        assert_eq!(rng1.random_u64(), rng2.random_u64());
        assert_eq!(rng1.random_string(10), rng2.random_string(10));
    }

    #[test]
    fn test_different_seeds_different_output() {
        let mut rng1 = DeterministicRng::new(42);
        let mut rng2 = DeterministicRng::new(43);

        assert_ne!(rng1.random_u64(), rng2.random_u64());
    }

    #[test]
    fn test_reset_functionality() {
        let mut rng = DeterministicRng::new(42);
        let first_value = rng.random_u64();
        let second_value = rng.random_u64();

        rng.reset();
        let reset_first = rng.random_u64();
        let reset_second = rng.random_u64();

        assert_eq!(first_value, reset_first);
        assert_eq!(second_value, reset_second);
    }

    #[test]
    fn test_various_generation_methods() {
        let mut rng = DeterministicRng::new(42);

        let string = rng.random_string(10);
        assert_eq!(string.len(), 10);
        assert!(string.chars().all(|c| c.is_ascii_lowercase()));

        let alphanum = rng.random_alphanumeric(8);
        assert_eq!(alphanum.len(), 8);
        assert!(alphanum.chars().all(|c| c.is_ascii_alphanumeric()));

        let range_val = rng.random_range_u32(1, 10);
        assert!((1..10).contains(&range_val));

        let bool_val = rng.random_bool();
        // Test that random_bool returns without panicking and produces a valid bool
        assert!(matches!(bool_val, true | false));
    }
}
