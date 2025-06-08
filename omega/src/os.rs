//! Random K-Combination Generator
//! 
//! This module provides efficient algorithms for generating random k-combinations
//! of numbers from 1 to n. The implementation uses a telescoping window algorithm
//! that guarantees O(k) time complexity and O(1) space complexity (excluding output).
//! 
//! # Features
//! - Generates combinations in sorted order
//! - No duplicate elements
//! - Single pass through the combination space
//! - Zero-copy operations where possible
//! - Optimized for both small and large values of k and n

use crate::orrg::OmegaRng;

/// Generates a random k-combination of numbers from 1 to n.
///
/// This function uses the telescoping window algorithm to efficiently generate
/// k distinct numbers in the range [1, n] in ascending order.
///
/// # Arguments
/// * `n` - Upper bound of the range (inclusive)
/// * `k` - Number of elements to choose
/// * `rng_state` - Mutable reference to the random number generator state
///
/// # Panics
/// * If `k` is zero or greater than `n`
/// * If `n` is zero
///
/// # Examples
/// ```
/// use your_crate_name::os::random_k_combination;
///
/// let mut rng_state = 42;
/// let combination = random_k_combination(10, 3, &mut rng_state);
/// assert_eq!(combination.len(), 3);
/// assert!(combination.windows(2).all(|w| w[0] < w[1])); // Check sorted
/// ```
pub fn random_k_combination(n: u64, k: u64, rng_state: &mut u64) -> Vec<u64> {
    debug_assert!(k <= n && k > 0 && n > 0, "k must be in range [1, n] and n > 0");
    
    if k == n {
        return (1..=n).collect();
    }
    
    let mut rng = OmegaRng::new(*rng_state);
    
    if k == 1 {
        return vec![rng.range(1, n)];
    }
    
    let mut result = Vec::with_capacity(k as usize);
    let (mut prev, mut boundary) = (0, n - k);
    
    for _ in 0..k {
        let next = rng.range(prev + 1, boundary);
        result.push(next);
        prev = next;
        boundary += 1;
    }
    
    result
}

/// Generates a zero-indexed random k-combination of numbers from 0 to n-1.
///
/// This is a zero-indexed variant of `random_k_combination`.
///
/// # Arguments
/// * `n` - Upper bound of the range (exclusive)
/// * `k` - Number of elements to choose
/// * `rng_state` - Mutable reference to the random number generator state
///
/// # Examples
/// ```
/// use your_crate_name::os::random_k_combination_zero_indexed;
///
/// let mut rng_state = 42;
/// let combination = random_k_combination_zero_indexed(10, 3, &mut rng_state);
/// assert_eq!(combination.len(), 3);
/// assert!(combination.windows(2).all(|w| w[0] < w[1]));
/// ```
#[inline]
pub fn random_k_combination_zero_indexed(n: u64, k: u64, rng_state: &mut u64) -> Vec<u64> {
    debug_assert!(k <= n && k > 0 && n > 0, "k must be in range [1, n] and n > 0");
    
    if k == n {
        return (0..n).collect();
    }
    
    let mut rng = OmegaRng::new(*rng_state);
    
    if k == 1 {
        return vec![rng.range(0, n - 1)];
    }
    
    let mut result = Vec::with_capacity(k as usize);
    let (mut prev, mut boundary) = (u64::MAX, n - k);
    
    for _ in 0..k {
        let next = rng.range(prev.wrapping_add(1), boundary);
        result.push(next);
        prev = next;
        boundary += 1;
    }
    
    result
}

/// Optimized version of random k-combination generator.
///
/// This version includes manual loop unrolling and other optimizations
/// for better performance in release builds.
///
/// # Arguments
/// * `rng_state` - Mutable reference to the random number generator state
/// * `n` - Upper bound of the range (inclusive)
/// * `k` - Number of elements to choose
///
/// # Examples
/// ```
/// use your_crate_name::os::random_k_combination_optimized;
///
/// let mut rng_state = 42;
/// let combination = random_k_combination_optimized(&mut rng_state, 10, 3);
/// assert_eq!(combination.len(), 3);
/// ```
#[inline]
pub fn random_k_combination_optimized(rng_state: &mut u64, n: u64, k: u64) -> Vec<u64> {
    debug_assert!(k <= n && k > 0 && n > 0, "k must be in range [1, n] and n > 0");
    
    let mut rng = OmegaRng::new(*rng_state);
    
    match (k, n) {
        (1, _) => vec![rng.range(1, n)],
        (k, n) if k == n => (1..=n).collect(),
        (k, n) => {
            let mut result = Vec::with_capacity(k as usize);
            let (mut prev, mut boundary) = (0, n - k);
            
            // Unroll first iteration
            let first = rng.range(1, boundary);
            result.push(first);
            prev = first;
            boundary += 1;
            
            // Process remaining elements
            for _ in 1..k {
                let next = rng.range(prev + 1, boundary);
                result.push(next);
                prev = next;
                boundary += 1;
            }
            
            result
        }
    }
}