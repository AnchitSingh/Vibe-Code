

//! # Omega Random Number Generator - Biased Distribution Version
//!
//! Enhanced version with configurable bias toward the initial 25% of ranges
//! while maintaining full range coverage and excellent statistical properties.
//!
//! Key features:
//! - Biased range generation favoring first 25% (configurable)
//! - Multiple bias strategies for different use cases
//! - Maintains unbiased option for when needed
//! - Excellent performance characteristics maintained

use std::hint::black_box;

// Enhanced mixing constants with better avalanche properties
const OMEGA_MULT_A: u64 = 0x9e3779b97f4a7c15; // Golden ratio
const OMEGA_MULT_B: u64 = 0xc6a4a7935bd1e995; // Strong avalanche multiplier
const OMEGA_MULT_C: u64 = 0xe7037ed1a0b428db; // Additional mixing constant

#[derive(Clone, Copy, Debug)]
pub struct OmegaRng {
    state_a: u64,
    state_b: u64,
    counter: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum BiasStrategy {
    /// No bias - uniform distribution
    None,
    /// Exponential decay - strong bias toward start
    Exponential,
    /// Weighted zones - 60% chance for first 25%, 40% for rest
    Weighted,
    /// Power curve - configurable curve steepness
    Power(f64),
    /// Stepped - higher probability in first quarter, uniform elsewhere
    Stepped,
}

impl OmegaRng {
    #[inline(always)]
    pub fn new(seed: u64) -> Self {
        // Enhanced initialization with more thorough mixing
        let mut z = seed.wrapping_add(OMEGA_MULT_A);
        
        // First round of mixing
        z = (z ^ (z >> 30)).wrapping_mul(OMEGA_MULT_B);
        z = (z ^ (z >> 27)).wrapping_mul(OMEGA_MULT_C);
        z = z ^ (z >> 31);
        let state_a = z;
        
        // Second round with different constants
        z = state_a.wrapping_add(OMEGA_MULT_A);
        z = (z ^ (z >> 29)).wrapping_mul(OMEGA_MULT_C);
        z = (z ^ (z >> 26)).wrapping_mul(OMEGA_MULT_B);
        z = z ^ (z >> 30);
        let state_b = z;
        
        // Third round for counter initialization
        z = state_b.wrapping_add(OMEGA_MULT_A);
        z = (z ^ (z >> 28)).wrapping_mul(OMEGA_MULT_B);
        let counter = z ^ (z >> 32);
        
        Self { state_a, state_b, counter }
    }
    
    #[inline(always)]
    fn next_raw(&mut self) -> u64 {
        let s0 = self.state_a;
        let mut s1 = self.state_b;
        
        // Enhanced XOROSHIRO with better mixing
        s1 ^= s0;
        self.state_a = s0.rotate_left(26) ^ s1 ^ (s1 << 9);
        self.state_b = s1.rotate_left(13);
        
        // Enhanced output function with stronger mixing
        let result = s0.wrapping_add(s1);
        let result = result ^ (result >> 31);
        let result = result.wrapping_mul(OMEGA_MULT_B);
        let result = result ^ (result >> 29);
        
        self.counter = self.counter.wrapping_add(1);
        result
    }
    
    /// Generate a random float in [0, 1) for bias calculations
    #[inline(always)]
    fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (1u64 << 53) as f64;
        ((self.next_raw() >> 11) as f64) * SCALE
    }
    
    /// Unbiased range generation (original method)
    #[inline(always)]
    pub fn range(&mut self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        
        let range = max - min + 1;
        let result = match range {
            1 => 0,
            2..=256 if range.is_power_of_two() => omega_range_pow2(self, range),
            2..=256 => omega_range_small(self, range),
            _ => omega_range_unbiased(self, range),
        };
        min + result
    }
    
    /// Biased range generation with configurable strategies
    #[inline(always)]
    pub fn range_biased(&mut self, min: u64, max: u64, bias: BiasStrategy) -> u64 {
        if min >= max {
            return min;
        }
        
        match bias {
            BiasStrategy::None => self.range(min, max),
            _ => self.range_biased_impl(min, max, bias),
        }
    }
    
    #[inline(always)]
    fn range_biased_impl(&mut self, min: u64, max: u64, bias: BiasStrategy) -> u64 {
        let full_range = max - min + 1;
        let quarter_size = full_range / 4;
        let quarter_end = min + quarter_size.saturating_sub(1);
        
        match bias {
            BiasStrategy::Weighted => {
                // 60% chance for first 25%, 40% for the rest
                if self.next_raw() % 100 < 60 {
                    // First quarter
                    self.range(min, quarter_end.min(max))
                } else {
                    // Rest of range
                    let rest_start = (min + quarter_size).min(max);
                    if rest_start > max {
                        min // Fallback if range is too small
                    } else {
                        self.range(rest_start, max)
                    }
                }
            },
            
            BiasStrategy::Exponential => {
                // Exponential decay bias - strong preference for early values
                let u = self.next_f64();
                let biased_u = 1.0 - (-u * 2.0).exp(); // Exponential curve
                let offset = (biased_u * full_range as f64) as u64;
                min + offset.min(full_range - 1)
            },
            
            BiasStrategy::Power(power) => {
                // Power curve bias - adjustable steepness
                let u = self.next_f64();
                let biased_u = u.powf(power);
                let offset = (biased_u * full_range as f64) as u64;
                min + offset.min(full_range - 1)
            },
            
            BiasStrategy::Stepped => {
                // Stepped bias - 3x probability for first quarter
                let choice = self.next_raw() % 4;
                if choice < 3 {
                    // First quarter (75% probability)
                    self.range(min, quarter_end.min(max))
                } else {
                    // Full range (25% probability)
                    self.range(min, max)
                }
            },
            
            BiasStrategy::None => unreachable!(),
        }
    }
    
    /// Convenience method for default 25% bias
    #[inline(always)]
    pub fn range_biased_25(&mut self, min: u64, max: u64) -> u64 {
        self.range_biased(min, max, BiasStrategy::Weighted)
    }
    
    
    /// Generates a random boolean value with the given probability of being `true`.
    ///
    /// # Arguments
    /// * `probability` - A floating-point value between 0.0 and 1.0 (inclusive) representing
    ///                  the probability of returning `true`. Values are clamped to the [0.0, 1.0] range.
    ///
    /// # Example
    /// ```
    /// # use crate::orrg::OmegaRng;
    /// let mut rng = OmegaRng::new(42);
    ///
    /// // 30% chance of true
    /// if rng.bool(0.3) {
    ///     println!("30% chance hit!");
    /// }
    ///
    /// // Classic 50/50 boolean
    /// if rng.bool(0.5) {
    ///     println!("Heads!");
    /// } else {
    ///     println!("Tails!");
    /// }
    /// ```
    #[inline(always)]
    pub fn bool(&mut self, probability: f64) -> bool {
        // Clamp probability to [0.0, 1.0] range
        let prob = probability.max(0.0).min(1.0);
        // Generate a random f64 in [0.0, 1.0) and compare with probability
        (self.f64() as f64) < prob
    }

    /// Generates a random boolean value with 50% probability.
    ///
    /// This is equivalent to `rng.bool(0.5)` but may be slightly more efficient.
    ///
    /// # Example
    /// ```
    /// # use crate::orrg::OmegaRng;
    /// let mut rng = OmegaRng::new(42);
    /// if rng.coin_flip() {
    ///     println!("Heads!");
    /// } else {
    ///     println!("Tails!");
    /// }
    /// ```
    #[inline(always)]
    pub fn coin_flip(&mut self) -> bool {
        self.next_raw() & 1 == 1
    }

    #[inline(always)]
    pub fn u8(&mut self) -> u8 {
        (self.next_raw() >> 56) as u8
    }

    #[inline(always)]
    pub fn u32(&mut self) -> u32 {
        (self.next_raw() >> 32) as u32
    }

    #[inline(always)]
    pub fn f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (1u64 << 53) as f64;
        ((self.next_raw() >> 11) as f64) * SCALE
    }

    /// Get internal counter (prevents optimization)
    #[inline(always)]
    pub fn get_counter(&self) -> u64 {
        self.counter
    }

    /// Shuffles a slice in-place using the Fisher-Yates algorithm.
    ///
    /// This method reorders the elements of the slice such that each possible permutation
    /// has equal probability of occurring.
    ///
    /// # Example
    /// ```
    /// # use crate::orrg::OmegaRng;
    /// let mut rng = OmegaRng::new(42);
    /// let mut numbers = vec![1, 2, 3, 4, 5];
    /// rng.shuffle(&mut numbers);
    /// // numbers is now in a random order
    /// ```
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            let j = self.range(0, i as u64) as usize;
            slice.swap(i, j);
        }
    }
    
    
}

// Keep original range functions for performance
#[inline(always)]
fn omega_range_unbiased(rng: &mut OmegaRng, range: u64) -> u64 {
    if range <= 1 {
        return 0;
    }
    
    let mut random = rng.next_raw();
    let mut multiresult = (random as u128) * (range as u128);
    let mut leftover = multiresult as u64;
    
    if leftover < range {
        let threshold = (0u64.wrapping_sub(range)) % range;
        while leftover < threshold {
            random = rng.next_raw();
            multiresult = (random as u128) * (range as u128);
            leftover = multiresult as u64;
        }
    }
    (multiresult >> 64) as u64
}

#[inline(always)]
fn omega_range_pow2(rng: &mut OmegaRng, range: u64) -> u64 {
    debug_assert!(range.is_power_of_two());
    rng.next_raw() & (range - 1)
}

#[inline(always)]
fn omega_range_small(rng: &mut OmegaRng, range: u64) -> u64 {
    if range.is_power_of_two() {
        return omega_range_pow2(rng, range);
    }
    
    let mask = range.next_power_of_two() - 1;
    loop {
        let candidate = rng.next_raw() & mask;
        if candidate < range {
            return candidate;
        }
    }
}

/// Convenience function for biased range with single state
#[inline(always)]
pub fn omega_rand_range_biased(state: &mut u64, min: u64, max: u64, bias: BiasStrategy) -> u64 {
    let mut rng = OmegaRng::new(*state);
    let result = rng.range_biased(min, max, bias);
    *state = rng.state_a ^ rng.state_b.rotate_left(32) ^ rng.counter;
    result
}

/// Original unbiased function maintained for compatibility
#[inline(always)]
pub fn omega_rand_range(state: &mut u64, min: u64, max: u64) -> u64 {
    omega_rand_range_biased(state, min, max, BiasStrategy::None)
}

// Test functions
#[allow(dead_code)]
fn test_bias_distribution() {
    println!("=== Testing Bias Distributions ===");
    
    let strategies = [
        ("Unbiased", BiasStrategy::None),
        ("Weighted (60/40)", BiasStrategy::Weighted),
        ("Exponential", BiasStrategy::Exponential),
        ("Power(0.5)", BiasStrategy::Power(0.5)),
        ("Power(2.0)", BiasStrategy::Power(2.0)),
        ("Stepped", BiasStrategy::Stepped),
    ];
    
    for (name, strategy) in strategies.iter() {
        let mut rng = OmegaRng::new(12345);
        let mut first_quarter = 0;
        let mut total = 0;
        const SAMPLES: u32 = 100_000;
        
        for _ in 0..SAMPLES {
            let val = rng.range_biased(0, 99, *strategy);
            total += 1;
            if val < 25 { // First 25% of 0-99 range
                first_quarter += 1;
            }
        }
        
        let bias_percentage = (first_quarter as f64 / total as f64) * 100.0;
        println!("{}: {:.1}% in first quarter (expected ~25% for unbiased)", 
                name, bias_percentage);
    }
}

#[allow(dead_code)]
fn test_bias_detailed_distribution() {
    println!("\n=== Detailed Distribution Analysis ===");
    let mut rng = OmegaRng::new(54321);
    let mut buckets = vec![0u32; 10];
    const SAMPLES: u32 = 100_000;
    
    // Test weighted bias on 0-9 range
    for _ in 0..SAMPLES {
        let val = rng.range_biased(0, 9, BiasStrategy::Weighted);
        buckets[val as usize] += 1;
    }
    
    println!("Weighted bias distribution (0-9 range):");
    for (i, &count) in buckets.iter().enumerate() {
        let percentage = (count as f64 / SAMPLES as f64) * 100.0;
        let bar = "█".repeat((percentage / 2.0) as usize);
        println!("Value {}: {:5} ({:5.2}%) {}", i, count, percentage, bar);
    }
    
    // First 2.5 values (25% of 0-9) should have higher probability
    let first_quarter_sum = buckets[0] + buckets[1] + (buckets[2] / 2); // Rough approximation
    let first_quarter_pct = (first_quarter_sum as f64 / SAMPLES as f64) * 100.0;
    println!("Approximate first 25% frequency: {:.1}%", first_quarter_pct);
}

#[allow(dead_code)]
fn benchmark_biased_performance() {
    println!("\n=== Performance Comparison ===");
    
    let strategies = [
        ("Unbiased", BiasStrategy::None),
        ("Weighted", BiasStrategy::Weighted),
        ("Exponential", BiasStrategy::Exponential),
        ("Power", BiasStrategy::Power(0.5)),
    ];
    
    for (name, strategy) in strategies.iter() {
        let mut rng = OmegaRng::new(12345);
        let start = std::time::Instant::now();
        let mut sum = 0u64;
        
        for _ in 0..1_000_000 {
            let val = rng.range_biased(0, 999, *strategy);
            sum = sum.wrapping_add(val);
            black_box(val);
        }
        
        let duration = start.elapsed();
        let ns_per_call = duration.as_nanos() as f64 / 1_000_000.0;
        
        println!("{}: {:.2}ns per call (sum: {})", name, ns_per_call, sum);
    }
}

#[allow(dead_code)]
fn test_practical_examples() {
    println!("\n=== Practical Examples ===");
    let mut rng = OmegaRng::new(98765);
    
    println!("Loot rarity (0=common, 9=legendary):");
    let mut loot_counts = vec![0u32; 10];
    for _ in 0..10_000 {
        let rarity = rng.range_biased(0, 9, BiasStrategy::Exponential);
        loot_counts[rarity as usize] += 1;
    }
    
    for (rarity, &count) in loot_counts.iter().enumerate() {
        let rarity_name = match rarity {
            0..=2 => "Common",
            3..=5 => "Uncommon", 
            6..=7 => "Rare",
            8 => "Epic",
            9 => "Legendary",
            _ => "Unknown",
        };
        println!("  {}: {} ({})", rarity, count, rarity_name);
    }
}

#[allow(dead_code)]
fn main() {
    println!("=== Omega RNG Biased Distribution Version ===");
    test_bias_distribution();
    test_bias_detailed_distribution();
    benchmark_biased_performance();
    test_practical_examples();
    
    println!("\n=== Usage Examples ===");
    println!("// Basic biased range (favors first 25%)");
    println!("let val = rng.range_biased_25(0, 100);");
    println!();
    println!("// Custom bias strategies");
    println!("let val = rng.range_biased(0, 100, BiasStrategy::Exponential);");
    println!("let val = rng.range_biased(0, 100, BiasStrategy::Power(0.3));");
    println!("let val = rng.range_biased(0, 100, BiasStrategy::Stepped);");
}