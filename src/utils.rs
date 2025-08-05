/// Global monotonic time source, initialized once at application startup.
///
/// This is used internally to track elapsed time since timer initialization.
/// Access is thread-safe after initialization.
static TIMER_START: OnceLock<Instant> = OnceLock::new();
use std::sync::OnceLock;
use std::time::Instant;

/// Initializes the global timer.
///
/// This function must be called exactly once before any timer-related
/// functions are used. Subsequent calls will return an error.
///
/// # Returns
/// - `Ok(())` if initialization was successful
/// - `Err(&'static str)` if the timer was already initialized
///
/// # Panics
/// Panics if called from multiple threads simultaneously.
pub fn timer_init() -> Result<(), &'static str> {
    TIMER_START
        .set(Instant::now())
        .map_err(|_| "Timer already initialized")
}

/// Returns the number of nanoseconds elapsed since timer initialization.
///
/// # Panics
/// Panics if called before `timer_init()`.
#[inline(always)]
pub fn elapsed_ns() -> u64 {
    let start = TIMER_START
        .get()
        .expect("Timer not initialized. Call timer_init() first.");
    Instant::now().duration_since(*start).as_nanos() as u64
}

const OMEGA_MULT_A: u64 = 0x9e3779b97f4a7c15;
const OMEGA_MULT_B: u64 = 0xc6a4a7935bd1e995;
const OMEGA_MULT_C: u64 = 0xe7037ed1a0b428db;

#[derive(Clone, Copy, Debug)]
pub struct VibeRng {
    state_a: u64,
    state_b: u64,
    counter: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum BiasStrategy {
    None,
    Exponential,
    Weighted,
    Power(f64),
    Stepped,
}

impl VibeRng {
    #[inline(always)]
    pub fn new(seed: u64) -> Self {
        let mut z = seed.wrapping_add(OMEGA_MULT_A);

        z = (z ^ (z >> 30)).wrapping_mul(OMEGA_MULT_B);
        z = (z ^ (z >> 27)).wrapping_mul(OMEGA_MULT_C);
        z = z ^ (z >> 31);
        let state_a = z;

        z = state_a.wrapping_add(OMEGA_MULT_A);
        z = (z ^ (z >> 29)).wrapping_mul(OMEGA_MULT_C);
        z = (z ^ (z >> 26)).wrapping_mul(OMEGA_MULT_B);
        z = z ^ (z >> 30);
        let state_b = z;

        z = state_b.wrapping_add(OMEGA_MULT_A);
        z = (z ^ (z >> 28)).wrapping_mul(OMEGA_MULT_B);
        let counter = z ^ (z >> 32);

        Self {
            state_a,
            state_b,
            counter,
        }
    }

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
    pub fn next_raw(&mut self) -> u64 {
        let s0 = self.state_a;
        let mut s1 = self.state_b;

        s1 ^= s0;
        self.state_a = s0.rotate_left(26) ^ s1 ^ (s1 << 9);
        self.state_b = s1.rotate_left(13);

        let result = s0.wrapping_add(s1);
        let result = result ^ (result >> 31);
        let result = result.wrapping_mul(OMEGA_MULT_B);
        let result = result ^ (result >> 29);

        self.counter = self.counter.wrapping_add(1);
        result
    }

    #[inline(always)]
    fn next_f64_fast(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (1u32 << 24) as f64;
        ((self.next_raw() >> 40) as u32 as f64) * SCALE
    }

    #[inline(always)]
    pub fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / (1u64 << 53) as f64;
        ((self.next_raw() >> 11) as f64) * SCALE
    }

    #[inline(always)]
    fn range_biased_impl(&mut self, min: u64, max: u64, bias: BiasStrategy) -> u64 {
        let full_range = max - min + 1;
        let quarter_size = full_range / 4;
        let quarter_end = min + quarter_size.saturating_sub(1);

        match bias {
            BiasStrategy::Weighted => {
                if self.next_raw() % 100 < 60 {
                    self.range(min, quarter_end.min(max))
                } else {
                    let rest_start = (min + quarter_size).min(max);
                    if rest_start > max {
                        min
                    } else {
                        self.range(rest_start, max)
                    }
                }
            }

            BiasStrategy::Exponential => {
                let u = self.next_f64_fast();
                let biased_u = (-u * 3.0).exp();
                let offset = (biased_u * full_range as f64) as u64;
                min + offset.min(full_range - 1)
            }

            BiasStrategy::Power(power) => {
                let u = self.next_f64_fast();
                let biased_u = if power < 1.0 {
                    1.0 - (1.0 - u).powf(1.0 / power)
                } else {
                    u.powf(power)
                };
                let offset = (biased_u * full_range as f64) as u64;
                min + offset.min(full_range - 1)
            }

            BiasStrategy::Stepped => {
                if (self.next_raw() & 3) < 3 {
                    self.range(min, quarter_end.min(max))
                } else {
                    self.range(min, max)
                }
            }

            BiasStrategy::None => unreachable!(),
        }
    }
}

#[inline(always)]
fn omega_range_unbiased(rng: &mut VibeRng, range: u64) -> u64 {
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
fn omega_range_pow2(rng: &mut VibeRng, range: u64) -> u64 {
    debug_assert!(range.is_power_of_two());
    rng.next_raw() & (range - 1)
}

#[inline(always)]
fn omega_range_small(rng: &mut VibeRng, range: u64) -> u64 {
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
