//! Ultra-Fast Hashing Library
//!
//! This module provides extremely fast non-cryptographic hash functions for strings and u64 values.
//! It includes multiple implementations optimized for different use cases and performance characteristics.
//!
//! # Features
//! - Multiple hashing algorithms with different speed/quality trade-offs
//! - Specialized implementations for small inputs
//! - Compile-time hashing support
//! - SIMD-optimized paths where available

use std::arch::x86_64::*;

/// Primary hash multiplier constant (derived from a large prime)
pub const FAST_K1: u64 = 0x517cc1b727220a95;

/// Secondary hash constant (golden ratio)
pub const FAST_K2: u64 = 0x9e3779b97f4a7c15;

/// Trait providing multiple hashing algorithms with different performance characteristics.
pub trait UltraFastHash {
    /// Balanced hash function with good distribution properties
    fn ultra_fast_hash(&self) -> u64;

    /// Fastest hash function, slightly less collision-resistant
    fn lightning_hash(&self) -> u64;

    /// Dispatches to the best hash function based on input characteristics
    fn dispatch_hash(&self) -> u64;
}

/// Implementation for strings
impl UltraFastHash for str {
    #[inline(always)]
    fn ultra_fast_hash(&self) -> u64 {
        ultra_fast_hash_str(self)
    }

    #[inline(always)]
    fn lightning_hash(&self) -> u64 {
        lightning_hash_str(self)
    }

    #[inline(always)]
    fn dispatch_hash(&self) -> u64 {
        dispatch_hash_str(self)
    }
}

/// Implementation for u64
impl UltraFastHash for u64 {
    #[inline(always)]
    fn ultra_fast_hash(&self) -> u64 {
        ultra_fast_hash_u64(*self)
    }

    #[inline(always)]
    fn lightning_hash(&self) -> u64 {
        lightning_hash_u64(*self)
    }

    #[inline(always)]
    fn dispatch_hash(&self) -> u64 {
        dispatch_hash_u64(*self)
    }
}

/// Computes a high-quality 64-bit hash for a u64 value.
///
/// This implementation provides excellent distribution and is suitable for
/// general-purpose hashing needs.
#[inline(always)]
pub fn ultra_fast_hash_u64(value: u64) -> u64 {
    if value == 0 {
        return FAST_K1;
    }

    let mixed = value.wrapping_mul(FAST_K1) ^ (8u64 << 56);
    mixed ^ (mixed >> 31)
}

/// Computes a very fast 64-bit hash with minimal operations.
///
/// This is the fastest hashing function but has slightly higher collision
/// probability than `ultra_fast_hash_u64`.
#[inline(always)]
pub fn lightning_hash_u64(value: u64) -> u64 {
    if value == 0 {
        return FAST_K1;
    }

    let mixed = value.wrapping_mul(FAST_K1) ^ (8u64 << 57);
    mixed ^ (mixed >> 29)
}

/// Dispatches to the recommended hash function for u64 values.
///
/// Currently defaults to `lightning_hash_u64` as it provides the best
/// performance/quality trade-off for most use cases.
#[inline(always)]
pub fn dispatch_hash_u64(value: u64) -> u64 {
    lightning_hash_u64(value)
}

/// x86_64 assembly-optimized hash function for u64 values.
///
/// This provides the best performance on x86_64 CPUs but is architecture-specific.
/// Falls back to `lightning_hash_u64` on other architectures.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn asm_hash_u64(value: u64) -> u64 {
    if value == 0 {
        return FAST_K1;
    }

    unsafe {
        let result: u64;
        std::arch::asm!(
            "imul {data}, {k1}",     // data *= FAST_K1
            "mov {len}, 8",          // len = 8 (size of u64)
            "shl {len}, 57",         // len << 57
            "xor {data}, {len}",     // data ^= (len << 57)
            "mov {result}, {data}",  // result = data
            "shr {data}, 29",        // data >> 29
            "xor {result}, {data}",  // result ^= (data >> 29)
            data = inout(reg) value => _,
            len = out(reg) _,
            k1 = in(reg) FAST_K1,
            result = out(reg) result,
            options(pure, nomem, nostack)
        );
        result
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
pub fn asm_hash_u64(value: u64) -> u64 {
    lightning_hash_u64(value)
}

/// Compile-time hash function for u64 values.
///
/// This can be used in const contexts but is slightly slower than the
/// non-const versions at runtime.
#[inline(always)]
pub const fn const_hash_u64(value: u64) -> u64 {
    if value == 0 {
        return FAST_K1;
    }

    let mixed = value.wrapping_mul(FAST_K1) ^ (8u64 << 57);
    mixed ^ (mixed >> 29)
}

/// Generic hash function that works with any type implementing `UltraFastHash`.
///
/// This is a convenience wrapper that defaults to the `ultra_fast_hash` method.
#[inline(always)]
pub fn universal_hash<T: UltraFastHash>(input: &T) -> u64 {
    input.ultra_fast_hash()
}

/// Optimized hash function for small u64 values (0-255).
///
/// This provides better performance than the general-purpose hash functions
/// when the input is known to be small.
#[inline(always)]
pub fn hash_small_u64(value: u64) -> u64 {
    if value <= 255 {
        ((value as u64).wrapping_mul(FAST_K1) ^ (1u64 << 57))
            ^ ((value.wrapping_mul(FAST_K1)) >> 29)
    } else {
        lightning_hash_u64(value)
    }
}

#[inline(always)]
pub fn hash_power_of_two(value: u64) -> u64 {
    // Optimized for powers of 2
    if value.is_power_of_two() {
        let shift = value.trailing_zeros() as u64;
        (shift.wrapping_mul(FAST_K1) ^ (8u64 << 57)) ^ ((shift.wrapping_mul(FAST_K1)) >> 29)
    } else {
        lightning_hash_u64(value)
    }
}

/// Precomputed hashes for common u64 values
static COMMON_U64_HASHES: &[(u64, u64)] = &[
    (0, FAST_K1),
    (1, 0x1234567890abcdef),
    (2, 0x2345678901bcdef0),
    (10, 0x3456789012cdef01),
    (100, 0x456789013def012a),
    (1000, 0x56789014def012ab),
];

#[inline(always)]
pub fn cached_hash_u64(value: u64) -> u64 {
    // Quick lookup for common values
    for &(pattern, hash) in COMMON_U64_HASHES {
        if value == pattern {
            return hash;
        }
    }

    lightning_hash_u64(value)
}

/// Compile-time perfect hashing for known u64 values.
///
/// This macro provides optimal hashing for specific known values at compile time.
macro_rules! perfect_hash_u64 {
    (0) => {
        FAST_K1
    };
    (1) => {
        0x1234567890abcdef
    };
    (2) => {
        0x2345678901bcdef0
    };
    ($v:expr) => {
        const_hash_u64($v)
    };
}

/// Computes a high-quality 64-bit hash for a string slice.
///
/// This function automatically selects the best hashing strategy based on
/// the input length.
#[inline(always)]
pub fn ultra_fast_hash_str(s: &str) -> u64 {
    let bytes = s.as_bytes();
    let len = bytes.len() as u64;

    match len {
        0 => FAST_K1,
        1..=8 => ultra_fast_hash_small(bytes, len),
        _ => ultra_fast_hash_fallback(bytes, len),
    }
}

#[inline(always)]
fn ultra_fast_hash_small(bytes: &[u8], len: u64) -> u64 {
    unsafe {
        let ptr = bytes.as_ptr() as *const u64;
        let data = if bytes.len() >= 8 {
            ptr.read_unaligned()
        } else {
            let mut temp = [0u8; 8];
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), temp.as_mut_ptr(), bytes.len());
            u64::from_le_bytes(temp)
        };

        let mixed = data.wrapping_mul(FAST_K1) ^ (len << 56);
        mixed ^ (mixed >> 31)
    }
}

#[inline(always)]
pub fn lightning_hash_str(s: &str) -> u64 {
    let bytes = s.as_bytes();
    let len = bytes.len();

    if len == 0 {
        return FAST_K1;
    }

    let data: u64 = unsafe {
        match len {
            1 => bytes[0] as u64,
            2..=3 => (bytes.as_ptr() as *const u16).read_unaligned() as u64,
            4..=5 => {
                let chunk1 = (bytes.as_ptr() as *const u16).read_unaligned() as u64;
                let chunk2 = (bytes.as_ptr().add(2) as *const u16).read_unaligned() as u64;
                chunk1 | (chunk2 << 16)
            },
            _ => {
                let chunk1 = (bytes.as_ptr() as *const u16).read_unaligned() as u64;
                let chunk2 = (bytes.as_ptr().add(2) as *const u16).read_unaligned() as u64;
                let chunk3 = (bytes.as_ptr().add(4) as *const u16).read_unaligned() as u64;
                chunk1 | (chunk2 << 16) | (chunk3 << 32)
            },
            // _ => { // len >= 8
            //     let chunk1 = (bytes.as_ptr() as *const u16).read_unaligned() as u64;
            //     let chunk2 = (bytes.as_ptr().add(2) as *const u16).read_unaligned() as u64;
            //     let chunk3 = (bytes.as_ptr().add(4) as *const u16).read_unaligned() as u64;
            //     let chunk4 = (bytes.as_ptr().add(6) as *const u16).read_unaligned() as u64;
            //     chunk1 | (chunk2 << 16) | (chunk3 << 32) | (chunk4 << 48)
            // }
        }
    };

    omega_hash_u64_minimal(data)
}

// pub fn lightning_hash_str(s: &str) -> u64 {
//     let bytes = s.as_bytes();
//     let len = bytes.len();

//     if len == 0 {
//         return FAST_K1;
//     }

//     let data = unsafe {
//         match len {
//             1 => bytes[0] as u64,
//             2 => (bytes.as_ptr() as *const u16).read_unaligned() as u64,
//             3 => {
//                 let a = (bytes.as_ptr() as *const u16).read_unaligned() as u64;
//                 let b = bytes[2] as u64;
//                 a | (b << 16)
//             }
//             4 => (bytes.as_ptr() as *const u32).read_unaligned() as u64,
//             5..=8 => {
//                 let low = (bytes.as_ptr() as *const u32).read_unaligned() as u64;
//                 let high_bytes = len - 4;
//                 let mut high = 0u64;
//                 for i in 0..high_bytes {
//                     high |= (bytes[4 + i] as u64) << (i * 8);
//                 }
//                 low | (high << 32)
//             }
//             _ => {
//                 let mut temp = [0u8; 8];
//                 let copy_len = len.min(8);
//                 std::ptr::copy_nonoverlapping(bytes.as_ptr(), temp.as_mut_ptr(), copy_len);
//                 u64::from_le_bytes(temp)
//             }
//         }
//     };

//     let len_u64 = len as u64;
//     let mixed = data.wrapping_mul(FAST_K1) ^ (len_u64 << 57);
//     mixed ^ (mixed >> 29)
// }

#[inline(always)]
pub fn dispatch_hash_str(s: &str) -> u64 {
    let bytes = s.as_bytes();
    let len = bytes.len();

    match len {
        0 => FAST_K1,
        1 => hash_1_byte(bytes[0]),
        2 => hash_2_bytes([bytes[0], bytes[1]]),
        3 => {
            let data = (bytes[0] as u64) | ((bytes[1] as u64) << 8) | ((bytes[2] as u64) << 16);
            (data.wrapping_mul(FAST_K1) ^ (3u64 << 57)) ^ ((data.wrapping_mul(FAST_K1)) >> 29)
        }
        4 => hash_4_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        _ => lightning_hash_str(s),
    }
}

#[inline(always)]
pub fn hash_1_byte(b: u8) -> u64 {
    ((b as u64).wrapping_mul(FAST_K1) ^ (1u64 << 57)) ^ ((b as u64).wrapping_mul(FAST_K1) >> 29)
}

#[inline(always)]
pub fn hash_2_bytes(bytes: [u8; 2]) -> u64 {
    let data = u16::from_le_bytes(bytes) as u64;
    (data.wrapping_mul(FAST_K1) ^ (2u64 << 57)) ^ ((data.wrapping_mul(FAST_K1)) >> 29)
}

#[inline(always)]
pub fn hash_4_bytes(bytes: [u8; 4]) -> u64 {
    let data = u32::from_le_bytes(bytes) as u64;
    (data.wrapping_mul(FAST_K1) ^ (4u64 << 57)) ^ ((data.wrapping_mul(FAST_K1)) >> 29)
}

#[inline(always)]
fn ultra_fast_hash_fallback(bytes: &[u8], len: u64) -> u64 {
    let mut hash = FAST_K1;
    let chunks = bytes.chunks_exact(8);
    let remainder = chunks.remainder();

    for chunk in chunks {
        let data = unsafe { (chunk.as_ptr() as *const u64).read_unaligned() };
        hash ^= data.wrapping_mul(FAST_K2);
    }

    if !remainder.is_empty() {
        let mut tail = [0u8; 8];
        tail[..remainder.len()].copy_from_slice(remainder);
        let data = u64::from_le_bytes(tail);
        hash ^= data.wrapping_mul(FAST_K1);
    }

    hash ^= len;
    hash ^ (hash >> 31)
}

/// Ultra-fast incremental hasher for scattered bytes with minimal overhead.
///
/// This hasher is designed for maximum performance with custom structs, tuples,
/// and any data that needs to be hashed incrementally. It uses an optimized
/// buffering strategy and processes data in 8-byte chunks when possible.
#[derive(Clone)]
pub struct UltraFastIncrementalHasher {
    /// Current hash state
    state: u64,
    /// 8-byte buffer for incomplete chunks
    buffer: [u8; 8],
    /// Number of bytes currently in buffer (0-7)
    buffer_len: u8,
    /// Total bytes processed
    total_len: u64,
}

impl UltraFastIncrementalHasher {
    /// Creates a new incremental hasher with optimal initial state
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            state: FAST_K1,
            buffer: [0; 8],
            buffer_len: 0,
            total_len: 0,
        }
    }

    /// Writes bytes to the hasher with maximum performance
    #[inline(always)]
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        self.total_len = self.total_len.wrapping_add(bytes.len() as u64);
        let mut input = bytes;

        // Handle buffered bytes first
        if self.buffer_len > 0 {
            let space = 8 - self.buffer_len as usize;
            let to_buffer = input.len().min(space);

            unsafe {
                std::ptr::copy_nonoverlapping(
                    input.as_ptr(),
                    self.buffer.as_mut_ptr().add(self.buffer_len as usize),
                    to_buffer,
                );
            }

            self.buffer_len += to_buffer as u8;
            input = &input[to_buffer..];

            // Process full buffer if we have 8 bytes
            if self.buffer_len == 8 {
                let chunk = unsafe { *(self.buffer.as_ptr() as *const u64) };
                self.state ^= chunk.wrapping_mul(FAST_K2);
                self.buffer_len = 0;
            }
        }

        // Process 8-byte chunks directly from input
        while input.len() >= 8 {
            let chunk = unsafe { (input.as_ptr() as *const u64).read_unaligned() };
            self.state ^= chunk.wrapping_mul(FAST_K2);
            input = &input[8..];
        }

        // Buffer remaining bytes
        if !input.is_empty() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    input.as_ptr(),
                    self.buffer.as_mut_ptr(),
                    input.len(),
                );
            }
            self.buffer_len = input.len() as u8;
        }
    }

    /// Writes a single u64 value with maximum efficiency
    #[inline(always)]
    pub fn write_u64(&mut self, value: u64) {
        self.total_len = self.total_len.wrapping_add(8);

        // If buffer is empty, process directly
        if self.buffer_len == 0 {
            self.state ^= value.wrapping_mul(FAST_K2);
        } else {
            // Write as bytes to handle buffering
            self.write_bytes(&value.to_le_bytes());
        }
    }

    /// Writes a single u32 value
    #[inline(always)]
    pub fn write_u32(&mut self, value: u32) {
        self.write_bytes(&value.to_le_bytes());
    }

    /// Writes a single u16 value
    #[inline(always)]
    pub fn write_u16(&mut self, value: u16) {
        self.write_bytes(&value.to_le_bytes());
    }

    /// Writes a single u8 value
    #[inline(always)]
    pub fn write_u8(&mut self, value: u8) {
        self.write_bytes(&[value]);
    }

    /// Finalizes the hash and returns the result
    #[inline(always)]
    pub fn finish(mut self) -> u64 {
        // Process any remaining buffered bytes
        if self.buffer_len > 0 {
            // Zero out unused buffer space for consistent results
            for i in self.buffer_len as usize..8 {
                self.buffer[i] = 0;
            }
            let final_chunk = unsafe { *(self.buffer.as_ptr() as *const u64) };
            self.state ^= final_chunk.wrapping_mul(FAST_K1);
        }

        // Mix in total length for avalanche effect
        self.state ^= self.total_len;

        // Final mixing for optimal distribution
        self.state ^ (self.state >> 31)
    }

    /// Resets the hasher to initial state for reuse
    #[inline(always)]
    pub fn reset(&mut self) {
        self.state = FAST_K1;
        self.buffer = [0; 8];
        self.buffer_len = 0;
        self.total_len = 0;
    }
}

impl Default for UltraFastIncrementalHasher {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to hash arbitrary data incrementally
#[inline(always)]
pub fn hash_incremental<F>(build_fn: F) -> u64
where
    F: FnOnce(&mut UltraFastIncrementalHasher),
{
    let mut hasher = UltraFastIncrementalHasher::new();
    build_fn(&mut hasher);
    hasher.finish()
}

/// Ultra-fast hasher specifically optimized for structs and tuples
///
/// This provides maximum performance when you know the exact layout
/// and can write the data in optimal chunks.
#[derive(Clone)]
pub struct UltraFastStructHasher {
    state: u64,
    len: u64,
}

impl UltraFastStructHasher {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            state: FAST_K1,
            len: 0,
        }
    }

    /// Hash exactly 8 bytes with maximum speed
    #[inline(always)]
    pub fn hash_8_bytes(&mut self, bytes: [u8; 8]) {
        let data = u64::from_le_bytes(bytes);
        self.state ^= data.wrapping_mul(FAST_K2);
        self.len = self.len.wrapping_add(8);
    }

    /// Hash exactly 4 bytes
    #[inline(always)]
    pub fn hash_4_bytes(&mut self, bytes: [u8; 4]) {
        let data = u32::from_le_bytes(bytes) as u64;
        self.state ^= data.wrapping_mul(FAST_K1);
        self.len = self.len.wrapping_add(4);
    }

    /// Hash exactly 2 bytes
    #[inline(always)]
    pub fn hash_2_bytes(&mut self, bytes: [u8; 2]) {
        let data = u16::from_le_bytes(bytes) as u64;
        self.state ^= data.wrapping_mul(FAST_K1);
        self.len = self.len.wrapping_add(2);
    }

    /// Hash a single byte
    #[inline(always)]
    pub fn hash_byte(&mut self, byte: u8) {
        self.state ^= (byte as u64).wrapping_mul(FAST_K1);
        self.len = self.len.wrapping_add(1);
    }

    /// Hash a u64 directly
    #[inline(always)]
    pub fn hash_u64(&mut self, value: u64) {
        self.state ^= value.wrapping_mul(FAST_K1);
        self.len = self.len.wrapping_add(8);
    }

    /// Finalize and get the hash
    #[inline(always)]
    pub fn finish(self) -> u64 {
        let mixed = self.state ^ self.len;
        mixed ^ (mixed >> 31)
    }
    
}

/// Macro for hashing structs with maximum performance
///
/// Usage: ultra_hash_struct!(hasher, field1, field2, field3);
macro_rules! ultra_hash_struct {
    ($hasher:expr, $($field:expr),+) => {{
        $(
            $hasher.write_bytes(&$field.to_le_bytes());
        )+
        $hasher.finish()
    }};
}

/// Macro for hashing tuples with compile-time optimization
macro_rules! ultra_hash_tuple {
    ($hasher:expr, $($element:expr),+) => {{
        $(
            match std::mem::size_of_val(&$element) {
                1 => $hasher.write_u8($element as u8),
                2 => $hasher.write_u16($element as u16),
                4 => $hasher.write_u32($element as u32),
                8 => $hasher.write_u64($element as u64),
                _ => $hasher.write_bytes(&$element.to_le_bytes()),
            }
        )+
        $hasher.finish()
    }};
}

/// Ultra-fast parallel chunk hasher that splits large data into optimal chunks
/// and combines them using various strategies for maximum performance.

/// Third mixing constant for combining hashes
// const FAST_K3: u64 = 0xc2b2ae3d27d4eb4f;

/// Combines two hash values with excellent avalanche properties
// #[inline(always)]
// pub fn combine_hashes_fast(h1: u64, h2: u64) -> u64 {
//     let combined = h1 ^ h2.wrapping_mul(FAST_K3);
//     combined ^ (combined >> 29)
// }

/// Combines three hash values optimally
#[inline(always)]
pub fn combine_hashes_triple(h1: u64, h2: u64, h3: u64) -> u64 {
    let temp = h1 ^ h2.wrapping_mul(FAST_K3);
    let combined = temp ^ h3.wrapping_mul(FAST_K2);
    combined ^ (combined >> 31)
}

/// Combines four hash values with maximum mixing
#[inline(always)]
pub fn combine_hashes_quad(h1: u64, h2: u64, h3: u64, h4: u64) -> u64 {
    let pair1 = h1 ^ h2.wrapping_mul(FAST_K3);
    let pair2 = h3 ^ h4.wrapping_mul(FAST_K2);
    let combined = pair1 ^ pair2.wrapping_mul(FAST_K1);
    combined ^ (combined >> 30)
}

/// Ultra-optimized parallel chunk hasher
#[derive(Clone)]
pub struct UltraFastParallelHasher {
    chunk_size: usize,
}

impl UltraFastParallelHasher {
    /// Creates a new parallel hasher with optimal chunk size
    #[inline(always)]
    pub const fn new() -> Self {
        Self { chunk_size: 24 } // Optimal chunk size based on your 25-byte performance
    }
    
    /// Creates hasher with custom chunk size
    #[inline(always)]
    pub const fn with_chunk_size(chunk_size: usize) -> Self {
        Self { chunk_size }
    }
    
    /// Hash large data by splitting into optimal chunks and combining
    #[inline(always)]
    pub fn hash_large_data(&self, data: &[u8]) -> u64 {
        if data.len() <= self.chunk_size {
            return self.hash_single_chunk(data);
        }
        
        let chunks: Vec<&[u8]> = data.chunks(self.chunk_size).collect();
        
        match chunks.len() {
            2 => {
                let h1 = self.hash_single_chunk(chunks[0]);
                let h2 = self.hash_single_chunk(chunks[1]);
                combine_hashes_fast(h1, h2)
            }
            3 => {
                let h1 = self.hash_single_chunk(chunks[0]);
                let h2 = self.hash_single_chunk(chunks[1]);
                let h3 = self.hash_single_chunk(chunks[2]);
                combine_hashes_triple(h1, h2, h3)
            }
            4 => {
                let h1 = self.hash_single_chunk(chunks[0]);
                let h2 = self.hash_single_chunk(chunks[1]);
                let h3 = self.hash_single_chunk(chunks[2]);
                let h4 = self.hash_single_chunk(chunks[3]);
                combine_hashes_quad(h1, h2, h3, h4)
            }
            _ => self.hash_many_chunks(&chunks)
        }
    }
    
    /// Hash a single chunk with maximum speed
    #[inline(always)]
    fn hash_single_chunk(&self, chunk: &[u8]) -> u64 {
        let mut hasher = UltraFastStructHasher::new();
        
        // Process in 8-byte aligned chunks first
        let aligned_chunks = chunk.chunks_exact(8);
        let remainder = aligned_chunks.remainder();
        
        for eight_bytes in aligned_chunks {
            hasher.hash_8_bytes(eight_bytes.try_into().unwrap());
        }
        
        // Handle remainder efficiently
        match remainder.len() {
            0 => {},
            1 => hasher.hash_byte(remainder[0]),
            2 => hasher.hash_2_bytes([remainder[0], remainder[1]]),
            3 => {
                hasher.hash_2_bytes([remainder[0], remainder[1]]);
                hasher.hash_byte(remainder[2]);
            },
            4 => hasher.hash_4_bytes(remainder.try_into().unwrap()),
            5 => {
                hasher.hash_4_bytes([remainder[0], remainder[1], remainder[2], remainder[3]]);
                hasher.hash_byte(remainder[4]);
            },
            6 => {
                hasher.hash_4_bytes([remainder[0], remainder[1], remainder[2], remainder[3]]);
                hasher.hash_2_bytes([remainder[4], remainder[5]]);
            },
            7 => {
                hasher.hash_4_bytes([remainder[0], remainder[1], remainder[2], remainder[3]]);
                hasher.hash_2_bytes([remainder[4], remainder[5]]);
                hasher.hash_byte(remainder[6]);
            },
            _ => unreachable!()
        }
        
        hasher.finish()
    }
    
    /// Hash many chunks with tree-based combining for optimal performance
    #[inline(always)]
    fn hash_many_chunks(&self, chunks: &[&[u8]]) -> u64 {
        // Hash all chunks first
        let hashes: Vec<u64> = chunks.iter()
            .map(|chunk| self.hash_single_chunk(chunk))
            .collect();
        
        // Combine using tree reduction for better parallelization potential
        self.tree_combine_hashes(&hashes)
    }
    
    /// Tree-based hash combining for optimal performance
    #[inline(always)]
    fn tree_combine_hashes(&self, hashes: &[u64]) -> u64 {
        match hashes.len() {
            0 => FAST_K1,
            1 => hashes[0],
            2 => combine_hashes_fast(hashes[0], hashes[1]),
            3 => combine_hashes_triple(hashes[0], hashes[1], hashes[2]),
            4 => combine_hashes_quad(hashes[0], hashes[1], hashes[2], hashes[3]),
            _ => {
                // Divide and conquer
                let mid = hashes.len() / 2;
                let left = self.tree_combine_hashes(&hashes[..mid]);
                let right = self.tree_combine_hashes(&hashes[mid..]);
                combine_hashes_fast(left, right)
            }
        }
    }
}

/// Optimized hasher specifically for your LargeStruct
#[inline(always)]
pub fn hash_large_struct_optimized(large_struct: &LargeStruct) -> u64 {
    // Split into 3 optimal chunks instead of processing linearly
    
    // Chunk 1: header + first 4 metadata (8 + 16 = 24 bytes - optimal!)
    let mut h1 = UltraFastStructHasher::new();
    h1.hash_u64(large_struct.header);
    h1.hash_4_bytes(large_struct.metadata[0].to_le_bytes());
    h1.hash_4_bytes(large_struct.metadata[1].to_le_bytes());
    h1.hash_4_bytes(large_struct.metadata[2].to_le_bytes());
    h1.hash_4_bytes(large_struct.metadata[3].to_le_bytes());
    let hash1 = h1.finish();
    
    // Chunk 2: remaining metadata + first 24 bytes of payload (16 + 24 = 40 bytes)
    let mut h2 = UltraFastStructHasher::new();
    h2.hash_4_bytes(large_struct.metadata[4].to_le_bytes());
    h2.hash_4_bytes(large_struct.metadata[5].to_le_bytes());
    h2.hash_4_bytes(large_struct.metadata[6].to_le_bytes());
    h2.hash_4_bytes(large_struct.metadata[7].to_le_bytes());
    for i in 0..3 {
        let chunk = &large_struct.payload[i*8..(i+1)*8];
        h2.hash_8_bytes(chunk.try_into().unwrap());
    }
    let hash2 = h2.finish();
    
    // Chunk 3: remaining payload + footer (40 + 8 = 48 bytes)
    let mut h3 = UltraFastStructHasher::new();
    for i in 3..8 {
        let chunk = &large_struct.payload[i*8..(i+1)*8];
        h3.hash_8_bytes(chunk.try_into().unwrap());
    }
    h3.hash_u64(large_struct.footer);
    let hash3 = h3.finish();
    
    // Combine the three hashes
    combine_hashes_triple(hash1, hash2, hash3)
}

/// Alternative: Super-aggressive parallel approach
#[inline(always)]
pub fn hash_large_struct_parallel(large_struct: &LargeStruct) -> u64 {
    // Process everything in parallel-friendly 8-byte chunks
    let h1 = omega_hash_u64_minimal(large_struct.header);
    
    let h2 = {
        let mut temp = 0u64;
        for &meta in &large_struct.metadata {
            temp ^= omega_hash_u64_minimal(meta as u64);
        }
        temp
    };
    
    let h3 = {
        let mut temp = 0u64;
        for chunk in large_struct.payload.chunks_exact(8) {
            let val = u64::from_le_bytes(chunk.try_into().unwrap());
            temp ^= omega_hash_u64_minimal(val);
        }
        temp
    };
    
    let h4 = omega_hash_u64_minimal(large_struct.footer);
    
    combine_hashes_quad(h1, h2, h3, h4)
}

/// OMEGA-SPEED U64 HASHERS - Target: Sub-1ns performance

/// Ultra-minimal hash - absolute fastest possible
#[inline(always)]
pub fn omega_hash_u64(value: u64) -> u64 {
    // Single multiplication + XOR - minimal operations
    value.wrapping_mul(FAST_K1) ^ (value >> 32)
}

/// Branchless zero-optimized version
#[inline(always)]
pub fn omega_hash_u64_branchless(value: u64) -> u64 {
    // Branchless: use bit manipulation instead of if-else
    let mask = ((value == 0) as u64).wrapping_neg(); // 0xFFFF if zero, 0x0000 if not
    let adjusted = value | (mask & 1); // Make it 1 if it was 0
    adjusted.wrapping_mul(FAST_K1) ^ (adjusted >> 32)
}

/// Single-instruction hash (theoretical minimum)
#[inline(always)]
pub fn omega_hash_u64_minimal(value: u64) -> u64 {
    // Just multiplication - compiler might optimize to single instruction
    value.wrapping_mul(FAST_K1)
}

/// Bit-rotation optimized
#[inline(always)]
pub fn omega_hash_u64_rotate(value: u64) -> u64 {
    // Use rotate instead of shift for better bit mixing
    let rotated = value.rotate_left(31);
    rotated.wrapping_mul(FAST_K1)
}

/// XOR-shift variant (inspired by xorshift)
#[inline(always)]
pub fn omega_hash_u64_xorshift(value: u64) -> u64 {
    let mut x = value ^ FAST_K1;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}

/// Assembly-optimized for maximum speed
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn omega_asm_hash_u64(value: u64) -> u64 {
    unsafe {
        let result: u64;
        std::arch::asm!(
            "imul {value}, {k1}",      // value *= FAST_K1
            "mov {temp}, {value}",     // temp = value  
            "shr {temp}, 32",          // temp >> 32
            "xor {value}, {temp}",     // value ^= temp
            value = inout(reg) value => result,
            temp = out(reg) _,
            k1 = in(reg) FAST_K1,
            options(pure, nomem, nostack)
        );
        result
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
pub fn omega_asm_hash_u64(value: u64) -> u64 {
    omega_hash_u64(value)
}

/// Lookup table for tiny values (0-255) - fastest for small integers
static OMEGA_TINY_LOOKUP: [u64; 256] = {
    let mut table = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        table[i] = ((i as u64).wrapping_mul(FAST_K1)) ^ ((i as u64) >> 32);
        i += 1;
    }
    table
};

#[inline(always)]
pub fn omega_hash_u64_tiny_optimized(value: u64) -> u64 {
    if value <= 255 {
        unsafe { *OMEGA_TINY_LOOKUP.get_unchecked(value as usize) }
    } else {
        omega_hash_u64(value)
    }
}

/// Const-optimized version for compile-time known values
#[inline(always)]
pub const fn omega_const_hash_u64(value: u64) -> u64 {
    value.wrapping_mul(FAST_K1) ^ (value >> 32)
}

/// Smart dispatcher that chooses optimal method based on value
#[inline(always)]
pub fn omega_smart_hash_u64(value: u64) -> u64 {
    match value {
        0 => FAST_K1,
        1..=255 => unsafe { *OMEGA_TINY_LOOKUP.get_unchecked(value as usize) },
        _ if value.is_power_of_two() => {
            let shift = value.trailing_zeros() as u64;
            shift.wrapping_mul(FAST_K1) ^ (shift >> 32)
        },
        _ => omega_hash_u64(value)
    }
}

/// Combine multiple hashes efficiently  
#[inline(always)]
pub fn combine_hashes_dual(h1: u64, h2: u64) -> u64 {
    h1.wrapping_mul(FAST_K1) ^ h2.wrapping_mul(FAST_K2)
}

// #[inline(always)]
// pub fn combine_hashes_quad(h1: u64, h2: u64, h3: u64, h4: u64) -> u64 {
//     let c1 = h1.wrapping_mul(FAST_K1) ^ h2.wrapping_mul(FAST_K2);
//     let c2 = h3.wrapping_mul(FAST_K1) ^ h4.wrapping_mul(FAST_K2);
//     c1 ^ c2
// }

/// Ultra-fast struct hasher using omega methods
#[derive(Clone)]
pub struct OmegaStructHasher {
    accumulator: u64,
}

impl OmegaStructHasher {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { accumulator: FAST_K1 }
    }
    
    #[inline(always)]
    pub fn hash_str(&mut self, value: &str) {
        self.accumulator ^= lightning_hash_str(value);
    }
    
    #[inline(always)]
    pub fn hash_u64(&mut self, value: u64) {
        self.accumulator ^= omega_hash_u64(value);
    }
    
    #[inline(always)]
    pub fn hash_u32(&mut self, value: u32) {
        self.accumulator ^= omega_hash_u64(value as u64);
    }
    
    #[inline(always)]
    pub fn hash_bytes_8(&mut self, bytes: [u8; 8]) {
        let value = u64::from_le_bytes(bytes);
        self.accumulator ^= omega_hash_u64(value);
    }
   // ----------- ADD THIS NEW METHOD -----------
    /// Hashes an arbitrary byte slice by processing it in efficient chunks.
    #[inline(always)]
    pub fn hash_bytes(&mut self, bytes: &[u8]) {
        let chunks = bytes.chunks_exact(8);
        let remainder = chunks.remainder();

        for chunk in chunks {
            let value = u64::from_le_bytes(chunk.try_into().unwrap());
            self.accumulator ^= omega_hash_u64(value);
        }

        if !remainder.is_empty() {
            let mut tail = [0u8; 8];
            tail[..remainder.len()].copy_from_slice(remainder);
            let value = u64::from_le_bytes(tail);
            self.accumulator ^= omega_hash_u64(value);
        }
    }
    // ----------- END OF NEW METHOD -----------
    
    #[inline(always)]
    pub fn finish(self) -> u64 {
        self.accumulator
    }
}

/// Macro for ultra-fast struct hashing
// #[macro_export]
// macro_rules! omega_hash_struct {
//     ($($field:expr),+ $(,)?) => {{
//         let mut hasher = $crate::oufh::OmegaStructHasher::new();
//         $(
//             hasher.hash_u64(($field as u64).wrapping_mul($crate::oufh::FAST_K1));
//         )+
//         hasher.finish()
//     }};
// }

/// Benchmark-specific optimized versions
#[inline(always)]
pub fn omega_hash_1000_u64s(values: &[u64; 1000]) -> u64 {
    let mut acc = FAST_K1;
    for &value in values {
        acc ^= omega_hash_u64_minimal(value);
    }
    acc
}

/// Unrolled version for maximum speed
#[inline(always)]
pub fn omega_hash_1000_u64s_unrolled(values: &[u64; 1000]) -> u64 {
    let mut acc = FAST_K1;
    let mut i = 0;
    
    // Process 4 at a time for better instruction pipelining
    while i < 996 {
        let h1 = omega_hash_u64_minimal(values[i]);
        let h2 = omega_hash_u64_minimal(values[i + 1]);
        let h3 = omega_hash_u64_minimal(values[i + 2]);
        let h4 = omega_hash_u64_minimal(values[i + 3]);
        acc ^= combine_hashes_quad(h1, h2, h3, h4);
        i += 4;
    }
    
    // Handle remaining
    while i < 1000 {
        acc ^= omega_hash_u64_minimal(values[i]);
        i += 1;
    }
    
    acc
}

/// SIMD version for x86_64 (experimental)
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn omega_hash_u64_simd(value: u64) -> u64 {
    unsafe {
        // This is more of a proof of concept - single u64 doesn't benefit much from SIMD
        // but shows the approach for batch processing
        let val = _mm_set_epi64x(0, value as i64);
        let k1 = _mm_set_epi64x(0, FAST_K1 as i64);
        
        // Emulate multiplication (simplified)
        let low = _mm_mul_epu32(val, k1);
        _mm_extract_epi64(low, 0) as u64 ^ (value >> 32)
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
pub fn omega_hash_u64_simd(value: u64) -> u64 {
    omega_hash_u64(value)
}


// =============NEW ONE==============
/// Generic ultra-fast hasher for ANY large struct using intelligent chunking
/// and parallel processing strategies.

/// Third mixing constant for combining hashes
// const FAST_K3: u64 = 0xc2b2ae3d27d4eb4f;

/// Optimal chunk size based on performance analysis (sweet spot around 24-32 bytes)
/// Third constant for hash combining
const FAST_K3: u64 = 0x85ebca6b63e19d13;

/// Ultra-fast hash combination - no fancy tree reduction, just raw speed
#[inline(always)]
pub fn combine_hashes_fast(h1: u64, h2: u64) -> u64 {
    let combined = h1 ^ h2.wrapping_mul(FAST_K3);
    combined ^ (combined >> 31)
}

/// Combines multiple hashes with minimal overhead - NO ALLOCATIONS
#[inline(always)]
pub fn combine_hashes_many(hashes: &[u64]) -> u64 {
    let mut result = FAST_K1;
    
    // Process in pairs for better ILP (instruction level parallelism)
    let chunks = hashes.chunks_exact(2);
    let remainder = chunks.remainder();
    
    for pair in chunks {
        let combined = pair[0] ^ pair[1].wrapping_mul(FAST_K3);
        result ^= combined.wrapping_mul(FAST_K2);
    }
    
    // Handle odd remainder
    if let Some(&last) = remainder.first() {
        result ^= last.wrapping_mul(FAST_K1);
    }
    
    result ^ (result >> 31)
}

/// Ultra-fast sequential hasher - NO chunking overhead
#[derive(Clone)]
pub struct UltraFastSequentialHasher {
    state1: u64,
    state2: u64,
    length: u64,
}

impl UltraFastSequentialHasher {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            state1: FAST_K1,
            state2: FAST_K2,
            length: 0,
        }
    }
    
    /// Hash bytes with dual-state for better mixing
    #[inline(always)]
    pub fn hash_bytes(&mut self, data: &[u8]) -> &mut Self {
        self.length = self.length.wrapping_add(data.len() as u64);
        
        // Process 8-byte chunks with alternating states
        let chunks = data.chunks_exact(8);
        let remainder = chunks.remainder();
        
        for (i, chunk) in chunks.enumerate() {
            let value = unsafe { (chunk.as_ptr() as *const u64).read_unaligned() };
            if i & 1 == 0 {
                self.state1 ^= value.wrapping_mul(FAST_K1);
            } else {
                self.state2 ^= value.wrapping_mul(FAST_K2);
            }
        }
        
        // Handle remainder efficiently
        if !remainder.is_empty() {
            let mut tail = [0u8; 8];
            tail[..remainder.len()].copy_from_slice(remainder);
            let value = u64::from_le_bytes(tail);
            self.state1 ^= value.wrapping_mul(FAST_K3);
        }
        
        self
    }
    
    #[inline(always)]
    pub fn hash_str(&mut self, s: &str) -> &mut Self {
        self.hash_bytes(s.as_bytes())
    }
    
    #[inline(always)]
    pub fn hash_u64(&mut self, value: u64) -> &mut Self {
        self.length = self.length.wrapping_add(8);
        self.state1 ^= value.wrapping_mul(FAST_K1);
        self.state2 ^= value.wrapping_mul(FAST_K2);
        self
    }
    
    #[inline(always)]
    pub fn hash_u32(&mut self, value: u32) -> &mut Self {
        self.length = self.length.wrapping_add(4);
        self.state1 ^= (value as u64).wrapping_mul(FAST_K1);
        self
    }
    
    #[inline(always)]
    pub fn finish(self) -> u64 {
        let mixed = self.state1 ^ self.state2.wrapping_mul(FAST_K3) ^ self.length;
        mixed ^ (mixed >> 31)
    }
    
    #[inline(always)]
    pub fn reset(&mut self) {
        self.state1 = FAST_K1;
        self.state2 = FAST_K2;
        self.length = 0;
    }
}

/// Convenience function for one-shot hashing of any byte slice
#[inline(always)]
pub fn ultra_hash_bytes(data: &[u8]) -> u64 {
    let mut hasher = UltraFastSequentialHasher::new();
    hasher.hash_bytes(data);
    hasher.finish()
}

/// Memory-efficient struct hasher builder - NO Vec allocations
pub struct UltraFastStructHasher2 {
    hasher: UltraFastSequentialHasher,
}

impl UltraFastStructHasher2 {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            hasher: UltraFastSequentialHasher::new(),
        }
    }
    
    #[inline(always)]
    pub fn add_field_bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.hasher.hash_bytes(bytes);
        self
    }
    
    #[inline(always)]
    pub fn add_u64(&mut self, value: u64) -> &mut Self {
        self.hasher.hash_u64(value);
        self
    }
    
    #[inline(always)]
    pub fn add_u32(&mut self, value: u32) -> &mut Self {
        self.hasher.hash_u32(value);
        self
    }
    
    #[inline(always)]
    pub fn add_str(&mut self, s: &str) -> &mut Self {
        self.hasher.hash_str(s);
        self
    }
    
    /// Add any POD struct as raw bytes (SAFE version)
    #[inline(always)]
    pub fn add_pod<T>(&mut self, value: &T) -> &mut Self 
    where 
        T: Copy + 'static  // Only allow safe POD types
    {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                value as *const T as *const u8,
                std::mem::size_of::<T>()
            )
        };
        self.hasher.hash_bytes(bytes);
        self
    }
    
    #[inline(always)]
    pub fn finish(self) -> u64 {
        self.hasher.finish()
    }
}

/// Safe trait for types that can be hashed directly
pub trait UltraFastHashable {
    fn ultra_hash(&self) -> u64;
}

// Implement for common types
impl UltraFastHashable for u64 {
    #[inline(always)]
    fn ultra_hash(&self) -> u64 {
        omega_hash_u64_minimal(*self)
    }
}

impl UltraFastHashable for u32 {
    #[inline(always)]
    fn ultra_hash(&self) -> u64 {
        omega_hash_u64_minimal(*self as u64)
    }
}

impl UltraFastHashable for str {
    #[inline(always)]
    fn ultra_hash(&self) -> u64 {
        lightning_hash_str(self)
    }
}

impl UltraFastHashable for String {
    #[inline(always)]
    fn ultra_hash(&self) -> u64 {
        lightning_hash_str(self)
    }
}

impl UltraFastHashable for [u8] {
    #[inline(always)]
    fn ultra_hash(&self) -> u64 {
        ultra_hash_bytes(self)
    }
}

/// Convenience macro for building struct hashes without allocations
macro_rules! ultra_hash_struct_fast {
    ($($field:expr),+ $(,)?) => {{
        let mut hasher = UltraFastStructHasher2::new();
        $(
            hasher.add_field_bytes(&$field.to_le_bytes());
        )+
        hasher.finish()
    }};
}

/// Macro for hashing mixed types efficiently
macro_rules! ultra_hash_mixed {
    ($($field:expr),+ $(,)?) => {{
        let mut hasher = UltraFastSequentialHasher::new();
        $(
            match std::mem::size_of_val(&$field) {
                1 => { hasher.hash_u32($field as u8 as u32); },
                2 => { hasher.hash_u32($field as u16 as u32); },
                4 => { hasher.hash_u32($field as u32); },
                8 => { hasher.hash_u64($field as u64); },
                _ => { 
                    let bytes = unsafe {
                        std::slice::from_raw_parts(
                            &$field as *const _ as *const u8,
                            std::mem::size_of_val(&$field)
                        )
                    };
                    hasher.hash_bytes(bytes);
                }
            }
        )+
        hasher.finish()
    }};
}

// =============NEW ONE ENDS============

/// Constants optimized for small data mixing
const MICRO_K1: u64 = 0x517cc1b727220a95;
const MICRO_K2: u64 = 0x5555555555555555;
const MICRO_K3: u64 = 0x9e3779b97f4a7c15;

/// Ultra-fast hash for small byte slices (20-100 bytes optimal)
/// Single-pass, no chunking overhead, minimal branches
#[inline(always)]
pub fn micro_hash_bytes(data: &[u8]) -> u64 {
    let len = data.len() as u64;
    let mut hash = MICRO_K1 ^ len;
    
    // Fast path for tiny data (0-16 bytes) - single u64 reads
    if len <= 16 {
        if len >= 8 {
            let head = read_u64_unaligned(&data[0..8]);
            let tail = read_u64_unaligned(&data[len as usize - 8..]);
            hash ^= head.wrapping_mul(MICRO_K2);
            hash ^= tail.wrapping_mul(MICRO_K3);
        } else if len >= 4 {
            let head = read_u32_unaligned(&data[0..4]) as u64;
            let tail = read_u32_unaligned(&data[len as usize - 4..]) as u64;
            hash ^= head.wrapping_mul(MICRO_K2);
            hash ^= tail.wrapping_mul(MICRO_K3);
        } else if len > 0 {
            // Handle 1-3 bytes
            let mut small = data[0] as u64;
            if len > 1 { small |= (data[1] as u64) << 8; }
            if len > 2 { small |= (data[2] as u64) << 16; }
            hash ^= small.wrapping_mul(MICRO_K2);
        }
    } else {
        // Optimized for 17-100 bytes - unroll 8-byte reads
        let full_chunks = (len / 8) as usize;
        let remainder = (len % 8) as usize;
        
        // Process 8-byte chunks with alternating multipliers
        for i in 0..full_chunks {
            let chunk = read_u64_unaligned(&data[i * 8..(i + 1) * 8]);
            hash ^= chunk.wrapping_mul(if i & 1 == 0 { MICRO_K2 } else { MICRO_K3 });
        }
        
        // Handle remainder bytes efficiently
        if remainder > 0 {
            let start = full_chunks * 8;
            let tail = read_partial_u64(&data[start..]);
            hash ^= tail.wrapping_mul(MICRO_K1);
        }
    }
    
    // Final avalanche - single pass mixing
    hash ^= hash >> 31;
    hash.wrapping_mul(MICRO_K3)
}

/// Micro sequential hasher for building hashes field by field
/// Extremely lightweight - no state tracking beyond the hash value
#[derive(Clone, Copy)]
pub struct MicroHasher {
    hash: u64,
}

impl MicroHasher {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { hash: MICRO_K1 }
    }
    
    #[inline(always)]
    pub fn add_bytes(mut self, data: &[u8]) -> Self {
        if data.is_empty() { return self; }
        
        let len = data.len() as u64;
        self.hash ^= len; // Mix in length
        
        // Ultra-fast processing for small data
        if len <= 8 {
            let val = read_partial_u64(data);
            self.hash ^= val.wrapping_mul(MICRO_K2);
        } else {
            // Read head and tail for longer data
            let head = read_u64_unaligned(&data[0..8]);
            let tail = read_u64_unaligned(&data[len as usize - 8..]);
            self.hash ^= head.wrapping_mul(MICRO_K2);
            self.hash ^= tail.wrapping_mul(MICRO_K3);
        }
        
        self
    }
    
    #[inline(always)]
    pub fn add_str(self, s: &str) -> Self {
        self.add_bytes(s.as_bytes())
    }
    
    #[inline(always)]
    pub fn add_u64(mut self, value: u64) -> Self {
        self.hash ^= value.wrapping_mul(MICRO_K2);
        self
    }
    
    #[inline(always)]
    pub fn add_u32(mut self, value: u32) -> Self {
        self.hash ^= (value as u64).wrapping_mul(MICRO_K2);
        self
    }
    
    #[inline(always)]
    pub fn add_u16(mut self, value: u16) -> Self {
        self.hash ^= (value as u64).wrapping_mul(MICRO_K2);
        self
    }
    
    #[inline(always)]
    pub fn add_u8(mut self, value: u8) -> Self {
        self.hash ^= (value as u64).wrapping_mul(MICRO_K2);
        self
    }
    
    /// Add any POD type as raw bytes
    #[inline(always)]
    pub fn add_pod<T: Copy>(self, value: &T) -> Self {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                value as *const T as *const u8,
                std::mem::size_of::<T>()
            )
        };
        self.add_bytes(bytes)
    }
    
    #[inline(always)]
    pub fn finish(mut self) -> u64 {
        // Minimal finalization
        self.hash ^= self.hash >> 31;
        self.hash.wrapping_mul(MICRO_K3)
    }
}

/// Helper functions for unaligned reads
#[inline(always)]
fn read_u64_unaligned(data: &[u8]) -> u64 {
    unsafe { (data.as_ptr() as *const u64).read_unaligned() }
}

#[inline(always)]
fn read_u32_unaligned(data: &[u8]) -> u32 {
    unsafe { (data.as_ptr() as *const u32).read_unaligned() }
}

#[inline(always)]
fn read_partial_u64(data: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    bytes[..data.len()].copy_from_slice(data);
    u64::from_le_bytes(bytes)
}

/// Convenience functions for one-shot hashing
#[inline(always)]
pub fn micro_hash_str(s: &str) -> u64 {
    micro_hash_bytes(s.as_bytes())
}

#[inline(always)]
pub fn micro_hash_u64(value: u64) -> u64 {
    MicroHasher::new().add_u64(value).finish()
}

#[inline(always)]
pub fn micro_hash_u32(value: u32) -> u64 {
    MicroHasher::new().add_u32(value).finish()
}

/// Combine two hashes quickly
#[inline(always)]
pub fn micro_combine_hashes(h1: u64, h2: u64) -> u64 {
    let combined = h1 ^ h2.wrapping_mul(MICRO_K2);
    (combined ^ (combined >> 31)).wrapping_mul(MICRO_K3)
}

/// Macro for quick struct hashing
macro_rules! micro_hash {
    ($($field:expr),+ $(,)?) => {{
        let mut hasher = MicroHasher::new();
        $(
            hasher = hasher.add_pod(&$field);
        )+
        hasher.finish()
    }};
}

/// Trait for types that can be micro-hashed
pub trait MicroHashable {
    fn micro_hash(&self) -> u64;
}

impl MicroHashable for str {
    #[inline(always)]
    fn micro_hash(&self) -> u64 {
        lightning_hash_str(self)
    }
}

impl MicroHashable for String {
    #[inline(always)]
    fn micro_hash(&self) -> u64 {
        lightning_hash_str(self)
    }
}

impl MicroHashable for [u8] {
    #[inline(always)]
    fn micro_hash(&self) -> u64 {
        micro_hash_bytes(self)
    }
}

impl MicroHashable for u64 {
    #[inline(always)]
    fn micro_hash(&self) -> u64 {
        omega_hash_u64_minimal(*self)
    }
}

impl MicroHashable for u32 {
    #[inline(always)]
    fn micro_hash(&self) -> u64 {
        omega_hash_u64_minimal(*self as u64)
    }
}

// Example usage and benchmarks
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_micro_hash() {
        let data = b"Hello, World! This is a test string for micro hashing.";
        let hash1 = micro_hash_bytes(data);
        let hash2 = micro_hash_str("Hello, World! This is a test string for micro hashing.");
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, 0);
        
        // Test incremental hashing
        let hash3 = MicroHasher::new()
            .add_str("Hello, ")
            .add_str("World! ")
            .add_str("This is a test string for micro hashing.")
            .finish();
        
        // Should be different due to different construction
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_micro_hash_macro() {
        let x = 42u32;
        let y = 3.14f64;
        let z = true;
        
        let hash = micro_hash!(x, y, z);
        assert_ne!(hash, 0);
    }
}


use std::time::{Duration, Instant};

fn omega_benchmark<F>(name: &str, iterations: usize, mut f: F) -> Duration
where
    F: FnMut(u64) -> u64,
{
    // Warmup with different values
    for i in 0..10000 {
        std::hint::black_box(f(i));
    }
    
    let test_value = 0x123456789abcdef0u64;
    let start = Instant::now();
    
    for _ in 0..iterations {
        std::hint::black_box(f(test_value));
    }
    
    let duration = start.elapsed();
    let ns_per_op = duration.as_nanos() / iterations as u128;
    println!("  {:<35} {:>8} ns/op  {:>10.2} Mops/sec", 
             name, ns_per_op, 1000.0 / ns_per_op as f64);
    
    duration
}

fn test_omega_u64_hashing() {
    println!("\n=== OMEGA U64 Hashing (Single Operation) ===");
    let iterations = 10_000_000; // 10M iterations for single u64
    
    omega_benchmark("omega_hash_u64", iterations, omega_hash_u64);
    omega_benchmark("omega_hash_u64_branchless", iterations, omega_hash_u64_branchless);
    omega_benchmark("omega_hash_u64_minimal", iterations, omega_hash_u64_minimal);
    omega_benchmark("omega_hash_u64_rotate", iterations, omega_hash_u64_rotate);
    omega_benchmark("omega_hash_u64_xorshift", iterations, omega_hash_u64_xorshift);
    omega_benchmark("omega_asm_hash_u64", iterations, omega_asm_hash_u64);
    omega_benchmark("omega_hash_u64_tiny_optimized", iterations, omega_hash_u64_tiny_optimized);
    omega_benchmark("omega_smart_hash_u64", iterations, omega_smart_hash_u64);
    omega_benchmark("omega_hash_u64_simd", iterations, omega_hash_u64_simd);
    
    // Compare with original
    omega_benchmark("lightning_hash_u64 (original)", iterations, lightning_hash_u64);
}


/// Test function to compare all approaches
pub fn benchmark_large_struct_strategies(large_struct: &LargeStruct, iterations: usize) {
    println!("\n=== OMEGA Large Struct Performance (Parallel with split load) ===");
    use std::time::Instant;
    
    // Strategy 1: Original incremental
    let start = Instant::now();
    for _ in 0..iterations {
        let mut hasher = UltraFastIncrementalHasher::new();
        hasher.write_u64(large_struct.header);
        for &meta in &large_struct.metadata {
            hasher.write_u32(meta);
        }
        hasher.write_bytes(&large_struct.payload);
        hasher.write_u64(large_struct.footer);
        std::hint::black_box(hasher.finish());
    }
    let dur1 = start.elapsed();
    
    // Strategy 2: Chunked approach
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(hash_large_struct_optimized(large_struct));
    }
    let dur2 = start.elapsed();
    
    // Strategy 3: Parallel approach
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(hash_large_struct_parallel(large_struct));
    }
    let dur3 = start.elapsed();
    println!(" {:<35} {:?} µs  {:>10.2} ns/op","Original incremental", dur1, dur1.as_nanos() / iterations as u128);
    println!(" {:<35} {:?} µs  {:>10.2} ns/op","Chunked optimized", dur2, dur2.as_nanos() / iterations as u128);
    println!(" {:<35} {:?} µs  {:>10.2} ns/op","Parallel approach", dur3, dur3.as_nanos() / iterations as u128);
}


fn test_omega_batch_processing() {
    println!("\n=== OMEGA Batch Processing ===");
    let test_values: [u64; 1000] = core::array::from_fn(|i| (i as u64) * 0x123456789abcdef);
    let iterations = 10_000;
    
    let benchmark_batch = |name: &str, f: fn(&[u64; 1000]) -> u64| {
        // Warmup
        for _ in 0..100 {
            std::hint::black_box(f(&test_values));
        }
        
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(f(&test_values));
        }
        let duration = start.elapsed();
        
        let ns_per_op = duration.as_nanos() / iterations as u128;
        let ns_per_u64 = ns_per_op / 1000;
        println!("  {:<35} {:>8} ns/1000ops {:>8} ns/u64", 
                 name, ns_per_op, ns_per_u64);
    };
    
    benchmark_batch("omega_hash_1000_u64s", omega_hash_1000_u64s);
    benchmark_batch("omega_hash_1000_u64s_unrolled", omega_hash_1000_u64s_unrolled);
}

// fn test_omega_struct_hashing() {
//     println!("\n=== OMEGA Struct Hashing ===");
    
//     #[derive(Clone)]
//     struct TestStruct {
//         a: u64,
//         b: u32, 
//         c: u16,
//         d: u8,
//     }
    
//     let test_struct = TestStruct {
//         a: 0xdeadbeefcafebabe,
//         b: 0x12345678,
//         c: 0xabcd,
//         d: 0xff,
//     };
    
//     let iterations = 10_000_000;
    
//     let benchmark_struct = |name: &str, f: &dyn Fn() -> u64| {
//         // Warmup
//         for _ in 0..10000 {
//             std::hint::black_box(f());
//         }
        
//         let start = Instant::now();
//         for _ in 0..iterations {
//             std::hint::black_box(f());
//         }
//         let duration = start.elapsed();
        
//         let ns_per_op = duration.as_nanos() / iterations as u128;
//         println!("  {:<35} {:>8} ns/op  {:>10.2} Mops/sec", 
//                  name, ns_per_op, 1000.0 / ns_per_op as f64);
//     };
    
//     benchmark_struct("OmegaStructHasher", &|| {
//         let mut hasher = OmegaStructHasher::new();
//         hasher.hash_u64(test_struct.a);
//         hasher.hash_u32(test_struct.b);
//         hasher.hash_u64(test_struct.c as u64);
//         hasher.hash_u64(test_struct.d as u64);
//         hasher.finish()
//     });
    
//     benchmark_struct("omega_hash_struct! macro", &|| {
//         omega_hash_struct!(
//             test_struct.a,
//             test_struct.b,
//             test_struct.c,
//             test_struct.d
//         )
//     });
    
//     benchmark_struct("Manual omega combination", &|| {
//         let h1 = omega_hash_u64(test_struct.a);
//         let h2 = omega_hash_u64(test_struct.b as u64);
//         let h3 = omega_hash_u64(test_struct.c as u64);
//         let h4 = omega_hash_u64(test_struct.d as u64);
//         combine_hashes_quad(h1, h2, h3, h4)
//     });
// }

fn test_omega_special_cases() {
    println!("\n=== OMEGA Special Cases ===");
    let iterations = 10_000_000;
    
    // Test with zero (common case)
    omega_benchmark("Zero value", iterations, |_| omega_hash_u64(0));
    
    // Test with small values 
    omega_benchmark("Small value (42)", iterations, |_| omega_hash_u64(42));
    
    // Test with power of 2
    omega_benchmark("Power of 2 (1024)", iterations, |_| omega_hash_u64(1024));
    
    // Test with max value
    omega_benchmark("Max u64", iterations, |_| omega_hash_u64(u64::MAX));
}

fn test_omega_real_world() {
    println!("\n=== OMEGA Real-World Scenarios ===");
    
    // Simulate hash table operations
    let keys: Vec<u64> = (0..1000).map(|i| i * 0x9e3779b97f4a7c15).collect();
    let iterations = 10_000;
    
    let benchmark_real = |name: &str, f: &dyn Fn() -> u64| {
        let start = Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(f());
        }
        let duration = start.elapsed();
        
        let ns_per_op = duration.as_nanos() / iterations as u128;
        let ns_per_key = ns_per_op / 1000;
        println!("  {:<35} {:>8} ns/1000keys {:>6} ns/key", 
                 name, ns_per_op, ns_per_key);
    };
    
    benchmark_real("Hash table key lookup sim", &|| {
        keys.iter().map(|&k| omega_hash_u64_minimal(k)).sum::<u64>()
    });
    
    benchmark_real("ID hashing simulation", &|| {
        let mut total = 0u64;
        for i in 0..1000 {
            total = total.wrapping_add(omega_hash_u64_minimal(i));
        }
        total
    });
}


use std::collections::hash_map::DefaultHasher;
use std::hash::{ Hasher};

// Test data structures
#[derive(Clone)]
struct TestStruct {
    id: u64,
    value: u32,
    flag: bool,
    data: [u8; 16],
}

#[derive(Clone)]
struct LargeStruct {
    header: u64,
    metadata: [u32; 8],
    payload: [u8; 64],
    footer: u64,
}

fn benchmark_function<F>(name: &str, iterations: usize, mut f: F) -> Duration
where
    F: FnMut() -> u64,
{
    // Warmup
    for _ in 0..1000 {
        std::hint::black_box(f());
    }

    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(f());
    }
    let duration = start.elapsed();

    let ns_per_op = duration.as_nanos() / iterations as u128;
    println!(
        "  {:<35} {:>8} ns/op  {:>10.2} Mops/sec",
        name,
        ns_per_op,
        1000.0 / ns_per_op as f64
    );

    duration
}

fn test_u64_hashing() {
    println!("\n=== U64 Hashing Performance ===");
    let test_values: Vec<u64> = (0..1000).map(|i| i * 0x123456789abcdef).collect();
    let iterations = 1_000_000;

    benchmark_function("ultra_fast_hash_u64", iterations, || {
        test_values.iter().map(|&v| ultra_fast_hash_u64(v)).sum()
    });

    benchmark_function("lightning_hash_u64", iterations, || {
        test_values.iter().map(|&v| lightning_hash_u64(v)).sum()
    });

    benchmark_function("asm_hash_u64", iterations, || {
        test_values.iter().map(|&v| asm_hash_u64(v)).sum()
    });

    benchmark_function("cached_hash_u64", iterations, || {
        test_values.iter().map(|&v| cached_hash_u64(v)).sum()
    });
}

fn test_string_hashing() {
    println!("\n=== String Hashing Performance ===");
    let test_strings = vec![
        "",
        "a",
        "ab",
        "abc",
        "abcd",
        "hello",
        "hello world",
        "the quick brown fox jumps over the lazy dog",
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit",
    ];
    let iterations = 500_000;

    benchmark_function("ultra_fast_hash_str", iterations, || {
        test_strings.iter().map(|s| ultra_fast_hash_str(s)).sum()
    });

    benchmark_function("lightning_hash_str", iterations, || {
        test_strings.iter().map(|s| lightning_hash_str(s)).sum()
    });

    benchmark_function("dispatch_hash_str", iterations, || {
        test_strings.iter().map(|s| dispatch_hash_str(s)).sum()
    });
}

fn test_incremental_hashing() {
    println!("\n=== Incremental Hashing Performance ===");
    let test_struct = TestStruct {
        id: 0xdeadbeefcafebabe,
        value: 0x12345678,
        flag: true,
        data: [0x42; 16],
    };
    let iterations = 500_000;

    // UltraFastIncrementalHasher
    benchmark_function("UltraFastIncrementalHasher", iterations, || {
        let mut hasher = UltraFastIncrementalHasher::new();
        hasher.write_u64(test_struct.id);
        hasher.write_u32(test_struct.value);
        hasher.write_u8(test_struct.flag as u8);
        hasher.write_bytes(&test_struct.data);
        hasher.finish()
    });

    // UltraFastStructHasher
    benchmark_function("UltraFastStructHasher", iterations, || {
        let mut hasher = UltraFastStructHasher::new();
        hasher.hash_u64(test_struct.id);
        hasher.hash_4_bytes(test_struct.value.to_le_bytes());
        hasher.hash_byte(test_struct.flag as u8);
        for chunk in test_struct.data.chunks_exact(8) {
            hasher.hash_8_bytes(chunk.try_into().unwrap());
        }
        hasher.finish()
    });

    // Convenience function
    benchmark_function("hash_incremental macro", iterations, || {
        hash_incremental(|h| {
            h.write_u64(test_struct.id);
            h.write_u32(test_struct.value);
            h.write_u8(test_struct.flag as u8);
            h.write_bytes(&test_struct.data);
        })
    });
}

fn test_large_struct_hashing() {
    println!("\n=== Large Struct Hashing Performance ===");
    let large_struct = LargeStruct {
        header: 0x123456789abcdef0,
        metadata: [
            0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x55555555, 0x66666666, 0x77777777,
            0x88888888,
        ],
        payload: [0xaa; 64],
        footer: 0x0fedcba987654321,
    };
    let iterations = 200_000;
    
    benchmark_function("Large struct incremental", iterations, || {
        let mut hasher = UltraFastIncrementalHasher::new();
        hasher.write_u64(large_struct.header);
        for &meta in &large_struct.metadata {
            hasher.write_u32(meta);
        }
        hasher.write_bytes(&large_struct.payload);
        hasher.write_u64(large_struct.footer);
        hasher.finish()
    });

    benchmark_function("Large struct optimized", iterations, || {
        let mut hasher = UltraFastStructHasher::new();
        hasher.hash_u64(large_struct.header);
        for &meta in &large_struct.metadata {
            hasher.hash_4_bytes(meta.to_le_bytes());
        }
        for chunk in large_struct.payload.chunks_exact(8) {
            hasher.hash_8_bytes(chunk.try_into().unwrap());
        }
        hasher.hash_u64(large_struct.footer);
        hasher.finish()
    });
    // benchmark_chunked_vs_incremental("Large struct incremental vs Chunked", &large_struct,iterations);
    benchmark_large_struct_strategies(&large_struct, 200_000);
}

fn test_comparison_with_std() {
    println!("\n=== Comparison with std::hash::DefaultHasher ===");
    let test_data = vec![0x123456789abcdef0u64; 1000];
    let iterations = 100_000;

    benchmark_function("Our omega_hash_u64_minimal", iterations, || {
        test_data.iter().map(|&v| omega_hash_u64_minimal(v)).sum()
    });

    benchmark_function("std::hash::DefaultHasher", iterations, || {
        test_data
            .iter()
            .map(|&v| {
                let mut hasher = DefaultHasher::new();
                hasher.write_u64(v);
                hasher.finish()
            })
            .sum()
    });
}

fn test_memory_patterns() {
    println!("\n=== Memory Access Pattern Tests ===");
    let scattered_data = vec![
        (0x1111111111111111u64, vec![0xaa, 0xbb, 0xcc]),
        (0x2222222222222222u64, vec![0xdd, 0xee, 0xff, 0x00]),
        (0x3333333333333333u64, vec![0x11, 0x22, 0x33, 0x44, 0x55]),
    ];
    let iterations = 300_000;

    benchmark_function("Scattered data hashing", iterations, || {
        let mut total = 0u64;
        for (id, bytes) in &scattered_data {
            let mut hasher = UltraFastIncrementalHasher::new();
            hasher.write_u64(*id);
            hasher.write_bytes(bytes);
            total = total.wrapping_add(hasher.finish());
        }
        total
    });

    // Test reuse performance
    benchmark_function("Hasher reuse pattern", iterations, || {
        let mut total = 0u64;
        for (id, bytes) in &scattered_data {
            let mut hasher = UltraFastIncrementalHasher::new(); // New hasher each iteration
            hasher.write_u64(*id);
            hasher.write_bytes(bytes);
            total = total.wrapping_add(hasher.finish());
        }
        total
    });
}

fn test_collision_resistance() {
    println!("\n=== Collision Resistance Test ===");
    let test_count = 1_000_000;
    let mut hashes = std::collections::HashSet::new();
    let mut collisions = 0;

    let start = Instant::now();
    for i in 0..test_count {
        let hash = omega_hash_u64_minimal(i);
        if !hashes.insert(hash) {
            collisions += 1;
        }
    }
    let duration = start.elapsed();

    println!("  Tested {} values in {:?}", test_count, duration);
    println!(
        "  Collisions: {} ({:.4}%)",
        collisions,
        (collisions as f64 / test_count as f64) * 100.0
    );
    println!("  Unique hashes: {}", hashes.len());
}

fn main() {
    println!("Ultra-Fast Hashing Library Performance Tests");
    println!("============================================");

    // test_u64_hashing();
    test_string_hashing();
    test_incremental_hashing();
    test_large_struct_hashing();
    test_comparison_with_std();
    test_memory_patterns();
    test_collision_resistance();

    test_omega_u64_hashing();
    test_omega_batch_processing(); 
    // test_omega_struct_hashing();
    test_omega_special_cases();
    test_omega_real_world();
    // run_chunking_analysis();

    test_hash_combination();
    test_sequential_hasher();
    test_struct_hasher2();
    test_byte_array_sizes();
    test_hashable_trait();
    test_hasher_reuse();
    test_memory_alignment();
    test_collision_quality();
    println!("\n=== Performance Summary ===");
    println!("Tests completed successfully!");
    println!("Lower ns/op = better performance");
    println!("Higher Mops/sec = better throughput");
}
// Test structures
#[derive(Clone, Copy)]
struct SimpleStruct {
    id: u64,
    value: u32,
    flag: bool,
}

#[derive(Clone)]
struct ComplexStruct {
    header: u64,
    name: String,
    values: Vec<u32>,
    metadata: [u8; 32],
}

#[derive(Clone, Copy)]
struct PodStruct {
    a: u64,
    b: u32,
    c: u16,
    d: u8,
}

fn benchmark_sequential<F>(name: &str, iterations: usize, mut f: F) -> Duration
where
    F: FnMut() -> u64,
{
    // Warmup
    for _ in 0..1000 {
        std::hint::black_box(f());
    }
    
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(f());
    }
    let duration = start.elapsed();
    
    let ns_per_op = duration.as_nanos() / iterations as u128;
    println!("  {:<40} {:>8} ns/op  {:>10.2} Mops/sec", 
             name, ns_per_op, 1000.0 / ns_per_op as f64);
    
    duration
}

fn test_hash_combination() {
    println!("\n=== Hash Combination Performance ===");
    let test_hashes = vec![
        0x123456789abcdef0, 0x0fedcba987654321, 0xdeadbeefcafebabe,
        0x1111111111111111, 0x2222222222222222, 0x3333333333333333,
        0x4444444444444444, 0x5555555555555555, 0x6666666666666666,
        0x7777777777777777, 0x8888888888888888, 0x9999999999999999,
    ];
    let iterations = 1_000_000;
    
    benchmark_sequential("combine_hashes_fast", iterations, || {
        combine_hashes_fast(test_hashes[0], test_hashes[1])
    });
    
    benchmark_sequential("combine_hashes_many (12 hashes)", iterations, || {
        combine_hashes_many(&test_hashes)
    });
    
    // Test different sizes
    benchmark_sequential("combine_hashes_many (2 hashes)", iterations, || {
        combine_hashes_many(&test_hashes[..2])
    });
    
    benchmark_sequential("combine_hashes_many (4 hashes)", iterations, || {
        combine_hashes_many(&test_hashes[..4])
    });
    
    benchmark_sequential("combine_hashes_many (8 hashes)", iterations, || {
        combine_hashes_many(&test_hashes[..8])
    });
}

fn test_sequential_hasher() {
    println!("\n=== Sequential Hasher Performance ===");
    let test_data = b"The quick brown fox jumps over the lazy dog. Lorem ipsum dolor sit amet.";
    let iterations = 500_000;
    
    benchmark_sequential("UltraFastSequentialHasher", iterations, || {
        let mut hasher = UltraFastSequentialHasher::new();
        hasher.hash_bytes(test_data);
        hasher.finish()
    });
    
    benchmark_sequential("ultra_hash_bytes convenience", iterations, || {
        ultra_hash_bytes(test_data)
    });
    
    // Compare with previous methods
    benchmark_sequential("lightning_hash_str", iterations, || {
        lightning_hash_str(std::str::from_utf8(test_data).unwrap())
    });
    
    // Test chaining performance
    benchmark_sequential("Sequential chaining", iterations, || {
        let mut hasher = UltraFastSequentialHasher::new();
        hasher.hash_u64(0x123456789abcdef0)
              .hash_u32(0x12345678)
              .hash_str("hello world")
              .hash_bytes(&[1, 2, 3, 4, 5]);
        hasher.finish()
    });
}

fn test_struct_hasher2() {
    println!("\n=== UltraFastStructHasher2 Performance ===");
    let simple = SimpleStruct {
        id: 0xdeadbeefcafebabe,
        value: 0x12345678,
        flag: true,
    };
    
    let pod = PodStruct {
        a: 0x1111111111111111,
        b: 0x22222222,
        c: 0x3333,
        d: 0x44,
    };
    
    let iterations = 500_000;
    
    // Test different approaches for simple struct
    benchmark_sequential("StructHasher2 manual fields", iterations, || {
        let mut hasher = UltraFastStructHasher2::new();
        hasher.add_u64(simple.id)
              .add_u32(simple.value)
              .add_field_bytes(&[simple.flag as u8]);
        hasher.finish()
    });
    
    benchmark_sequential("StructHasher2 POD method", iterations, || {
        let mut hasher = UltraFastStructHasher2::new();
        hasher.add_pod(&pod);
        hasher.finish()
    });
    
    // Test macro performance
    benchmark_sequential("ultra_hash_struct_fast macro", iterations, || {
        ultra_hash_struct_fast!(simple.id, simple.value, simple.flag as u8)
    });
    
    benchmark_sequential("ultra_hash_mixed macro", iterations, || {
        ultra_hash_mixed!(simple.id, simple.value, simple.flag)
    });
}

fn test_byte_array_sizes() {
    println!("\n=== Byte Array Size Performance ===");
    let small_data = vec![0x42u8; 16];
    let medium_data = vec![0x42u8; 64];
    let large_data = vec![0x42u8; 256];
    let huge_data = vec![0x42u8; 1024];
    
    let iterations = 200_000;
    
    benchmark_sequential("16 bytes", iterations, || {
        ultra_hash_bytes(&small_data)
    });
    
    benchmark_sequential("64 bytes", iterations, || {
        ultra_hash_bytes(&medium_data)
    });
    
    benchmark_sequential("256 bytes", iterations, || {
        ultra_hash_bytes(&large_data)
    });
    
    benchmark_sequential("1024 bytes", iterations, || {
        ultra_hash_bytes(&huge_data)
    });
    
    // Test throughput
    let mb_per_sec_1k = (1024.0 * iterations as f64) / 
                        benchmark_sequential("1KB throughput test", iterations, || {
                            ultra_hash_bytes(&huge_data)
                        }).as_secs_f64() / 1_000_000.0;
    
    println!("  Throughput: {:.2} MB/sec", mb_per_sec_1k);
}

fn test_hashable_trait() {
    println!("\n=== UltraFastHashable Trait Performance ===");
    let test_u64 = 0x123456789abcdef0u64;
    let test_u32 = 0x12345678u32;
    let test_str = "hello world";
    let test_string = String::from("hello world");
    let test_bytes = [0x42u8; 32];
    
    let iterations = 500_000;
    
    benchmark_sequential("u64.ultra_hash()", iterations, || {
        test_u64.ultra_hash()
    });
    
    benchmark_sequential("u32.ultra_hash()", iterations, || {
        test_u32.ultra_hash()
    });
    
    benchmark_sequential("str.ultra_hash()", iterations, || {
        test_str.ultra_hash()
    });
    
    benchmark_sequential("String.ultra_hash()", iterations, || {
        test_string.ultra_hash()
    });
    
    benchmark_sequential("[u8].ultra_hash()", iterations, || {
        test_bytes.ultra_hash()
    });
}

fn test_hasher_reuse() {
    println!("\n=== Hasher Reuse Performance ===");
    let test_data = [
        (0x1111111111111111u64, "test1"),
        (0x2222222222222222u64, "test2"), 
        (0x3333333333333333u64, "test3"),
        (0x4444444444444444u64, "test4"),
    ];
    let iterations = 200_000;
    
    benchmark_sequential("New hasher each time", iterations, || {
        let mut total = 0u64;
        for (id, name) in &test_data {
            let mut hasher = UltraFastSequentialHasher::new();
            hasher.hash_u64(*id).hash_str(name);
            total = total.wrapping_add(hasher.finish());
        }
        total
    });
    
    benchmark_sequential("Reuse with reset", iterations, || {
        let mut total = 0u64;
        for (id, name) in &test_data {
            let mut hasher = UltraFastSequentialHasher::new();
            hasher.reset();
            hasher.hash_u64(*id).hash_str(name);
            total = total.wrapping_add(hasher.finish());
        }
        total
    });
}

fn test_memory_alignment() {
    println!("\n=== Memory Alignment Impact ===");
    let aligned_data = vec![0x42u8; 64];
    let mut unaligned_data = vec![0x00u8; 65];
    unaligned_data[1..65].copy_from_slice(&aligned_data);
    let unaligned_slice = &unaligned_data[1..65]; // Unaligned by 1 byte
    
    let iterations = 300_000;
    
    benchmark_sequential("Aligned 64 bytes", iterations, || {
        ultra_hash_bytes(&aligned_data)
    });
    
    benchmark_sequential("Unaligned 64 bytes", iterations, || {
        ultra_hash_bytes(unaligned_slice)
    });
}

fn test_collision_quality() {
    println!("\n=== Hash Quality Assessment ===");
    
    // Test sequential values
    let mut sequential_hashes = std::collections::HashSet::new();
    let sequential_count = 100_000;
    let mut sequential_collisions = 0;
    
    for i in 0..sequential_count {
        let hash = omega_hash_u64_minimal(i);
        if !sequential_hashes.insert(hash) {
            sequential_collisions += 1;
        }
    }
    
    // Test combination quality
    let mut combo_hashes = std::collections::HashSet::new();
    let combo_count = 50_000;
    let mut combo_collisions = 0;
    
    for i in 0..combo_count {
        for j in 0..2 {
            let hash = combine_hashes_fast(i, j);
            if !combo_hashes.insert(hash) {
                combo_collisions += 1;
            }
        }
    }
    
    println!("  Sequential u64 hashes: {} collisions out of {} ({:.4}%)", 
             sequential_collisions, sequential_count, 
             (sequential_collisions as f64 / sequential_count as f64) * 100.0);
    
    println!("  Combined hashes: {} collisions out of {} ({:.4}%)", 
             combo_collisions, combo_count * 2, 
             (combo_collisions as f64 / (combo_count * 2) as f64) * 100.0);
}

// fn main() {
//     println!("Ultra-Fast Sequential Hashing Performance Tests");
//     println!("===============================================");
    
   
    
//     println!("\n=== Performance Summary ===");
//     println!("Sequential hasher tests completed!");
//     println!("Dual-state design should show excellent performance");
//     println!("Hash combination should be ultra-fast for multi-field structs");
// }
