//! A high-performance, memory-efficient hash set implementation with type specialization.
//!
//! This module provides `OmegaHashSet`, a hash-based collection that uses different
//! storage strategies based on the number of elements in each bucket. It features:
//! - Inline storage for small buckets (up to 3 elements)
//! - Overflow vectors for larger buckets
//! - Type specialization for String and u64 keys
//! - Unsafe optimizations for performance-critical paths

use crate::oufh::{lightning_hash_str, UltraFastHash};
use crate::orrg::OmegaRng;

/// Maximum number of elements stored directly in the bucket before using a vector
const INLINE_BUCKET_SIZE: usize = 3;

/// Initial capacity for overflow vectors when converting from inline storage
const OVERFLOW_BUCKET_SIZE: usize = 8;

/// Internal bucket storage that can be either empty, inline (fixed-size array),
/// or overflow (dynamic vector).
#[derive(Debug, Clone)]
enum OmegaBucket<K, V> {
    /// Empty bucket state
    Empty,
    
    /// Inline storage for small numbers of elements
    Inline {
        entries: [(K, V); INLINE_BUCKET_SIZE],
        len: u8,
    },
    
    /// Overflow storage for larger numbers of elements
    Overflow {
        entries: Vec<(K, V)>,
    },
}

impl<K, V> Default for OmegaBucket<K, V> 
where 
    K: Clone + Default,
    V: Clone + Default,
{
    fn default() -> Self {
        OmegaBucket::Empty
    }
}

/// A high-performance hash set with type specialization and memory efficiency.
///
/// # Type Parameters
/// - `K`: Key type (must implement `Eq + Clone + Default`)
/// - `V`: Value type (implements `Clone + Default`, defaults to `()` for set-like behavior)
///
/// # Performance Characteristics
/// - O(1) average case for insertions and lookups
/// - Memory usage scales with number of elements and bucket distribution
/// - Optimized for both small and large numbers of elements
#[derive(Debug)]
pub struct OmegaHashSet<K, V = ()>
where
    K: Eq + Clone + Default,
    V: Clone + Default,
{
    /// Primary storage for hash buckets
    storage: Vec<OmegaBucket<K, V>>,
    
    /// Total number of key-value pairs in the map
    count: usize,
}

// Implementation of generic methods for all OmegaHashSet<K, V>
impl<K, V> OmegaHashSet<K, V>
where
    K: Eq + Clone + Default,
    V: Clone + Default,
{
    /// Creates a new, empty OmegaHashSet with the specified table size.
    ///
    /// # Arguments
    /// * `table_size` - The initial number of buckets in the hash table.
    ///                  Should be a prime number for best distribution.
    fn new_internal(table_size: usize) -> Self {
        OmegaHashSet {
            storage: vec![OmegaBucket::Empty; table_size],
            count: 0,
        }
    }
    
    /// Returns the number of elements in the set.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }
    
    /// Returns `true` if the set contains no elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Implementation specialized for String keys with optimized string handling.
///
/// This specialization provides optimized implementations for string keys,
/// including direct string slice comparisons and efficient hashing.
impl<V> OmegaHashSet<String, V>
where
    V: Clone + Default,
{
    /// Creates a new, empty OmegaHashSet optimized for String keys.
    ///
    /// # Arguments
    /// * `table_size` - The initial number of buckets in the hash table.
    ///                  Should be a prime number for best distribution.
    pub fn new_string_map(table_size: usize) -> Self { 
        Self::new_internal(table_size) 
    }

    /// Computes the bucket index for a given key.
    ///
    /// # Arguments
    /// * `key` - The string key to compute the rank for
    ///
    /// # Returns
    /// A bucket index in the range [0, table_size-1]
    #[inline(always)]
    fn get_rank_for_key(&self, key: &str) -> usize {
        let seed = lightning_hash_str(key);
        let mut rng = OmegaRng::new(seed);
        let rank = rng.range(0, self.storage.len() as u64 - 1);
        rank as usize
    }

    pub fn insert<Q>(&mut self, key: Q, value: V) -> Option<V>
    where
        Q: Into<String> + AsRef<str>,
    {
        let key_str = key.as_ref();
        let rank_idx = self.get_rank_for_key(key_str);
        
        // OMEGA OPTIMIZATION: Direct unsafe access - no bounds checking
        let bucket = unsafe { self.storage.get_unchecked_mut(rank_idx) };
        
        match bucket {
            OmegaBucket::Empty => {
                // OMEGA OPTIMIZATION: Use MaybeUninit to avoid default string creation
                use std::mem::MaybeUninit;
                
                let mut entries: [MaybeUninit<(String, V)>; INLINE_BUCKET_SIZE] = 
                    unsafe { MaybeUninit::uninit().assume_init() };
                
                // Only initialize the first slot
                entries[0].write((key.into(), value));
                
                // Initialize remaining slots with defaults (unavoidable for now)
                for i in 1..INLINE_BUCKET_SIZE {
                    entries[i].write((String::new(), V::default()));
                }
                
                // SAFETY: All entries are now initialized
                let entries = unsafe {
                    let mut result = std::mem::MaybeUninit::<[(String, V); INLINE_BUCKET_SIZE]>::uninit();
                    let ptr = result.as_mut_ptr() as *mut (String, V);
                    
                    for i in 0..INLINE_BUCKET_SIZE {
                        ptr.add(i).write(entries[i].assume_init_read());
                    }
                    result.assume_init()
                };
                
                *bucket = OmegaBucket::Inline { entries, len: 1 };
                self.count += 1;
                None
            }
            OmegaBucket::Inline { entries, len } => {
                let current_len = *len as usize;
                
                // OMEGA OPTIMIZATION: Unrolled search with string slice comparison
                for i in 0..current_len {
                    let entry = unsafe { entries.get_unchecked_mut(i) };
                    if entry.0.as_str() == key_str { // Compare as &str, not String
                        return Some(std::mem::replace(&mut entry.1, value));
                    }
                }
                
                // Add new entry if space available
                if current_len < INLINE_BUCKET_SIZE {
                    // OMEGA OPTIMIZATION: Direct assignment, no bounds checking
                    unsafe {
                        let entry = entries.get_unchecked_mut(current_len);
                        entry.0 = key.into();
                        entry.1 = value;
                    }
                    *len += 1;
                    self.count += 1;
                    None
                } else {
                    // OMEGA OPTIMIZATION: Pre-allocate with exact capacity
                    let mut overflow_vec = Vec::with_capacity(OVERFLOW_BUCKET_SIZE);
                    
                    // Move existing entries efficiently
                    for entry in entries.iter_mut() {
                        overflow_vec.push((
                            std::mem::take(&mut entry.0), 
                            std::mem::take(&mut entry.1)
                        ));
                    }
                    overflow_vec.push((key.into(), value));
                    
                    *bucket = OmegaBucket::Overflow { entries: overflow_vec };
                    self.count += 1;
                    None
                }
            }
            OmegaBucket::Overflow { entries } => {
                // OMEGA OPTIMIZATION: Fast search with &str comparison
                for entry in entries.iter_mut() {
                    if entry.0.as_str() == key_str {
                        return Some(std::mem::replace(&mut entry.1, value));
                    }
                }
                
                // OMEGA OPTIMIZATION: Only reserve when at capacity
                if entries.len() == entries.capacity() {
                    entries.reserve(entries.len()); // Double capacity
                }
                entries.push((key.into(), value));
                self.count += 1;
                None
            }
        }
    }

    #[inline(always)]
    pub fn get<Q>(&self, key: &Q) -> Option<&V> 
    where
        Q: AsRef<str> + ?Sized,
    {
        let key_str = key.as_ref();
        let rank_idx = self.get_rank_for_key(key_str);
        
        // OMEGA OPTIMIZATION: Unsafe access for maximum speed
        let bucket = unsafe { self.storage.get_unchecked(rank_idx) };
        
        match bucket {
            OmegaBucket::Empty => None,
            OmegaBucket::Inline { entries, len } => {
                let current_len = *len as usize;
                // OMEGA OPTIMIZATION: Manual loop unrolling for common cases
                match current_len {
                    1 => {
                        let (k, v) = unsafe { entries.get_unchecked(0) };
                        if k.as_str() == key_str { Some(v) } else { None }
                    },
                    2 => {
                        let (k1, v1) = unsafe { entries.get_unchecked(0) };
                        if k1.as_str() == key_str { return Some(v1); }
                        let (k2, v2) = unsafe { entries.get_unchecked(1) };
                        if k2.as_str() == key_str { Some(v2) } else { None }
                    },
                    3 => {
                        let (k1, v1) = unsafe { entries.get_unchecked(0) };
                        if k1.as_str() == key_str { return Some(v1); }
                        let (k2, v2) = unsafe { entries.get_unchecked(1) };
                        if k2.as_str() == key_str { return Some(v2); }
                        let (k3, v3) = unsafe { entries.get_unchecked(2) };
                        if k3.as_str() == key_str { Some(v3) } else { None }
                    },
                    _ => None
                }
            }
            OmegaBucket::Overflow { entries } => {
                // OMEGA OPTIMIZATION: Efficient iteration with early termination
                for (k, v) in entries.iter() {
                    if k.as_str() == key_str { 
                        return Some(v); 
                    }
                }
                None
            }
        }
    }

        /// Efficiently inserts multiple key-value pairs in a batch.
    ///
    /// This is more efficient than individual inserts as it can optimize
    /// memory allocations and hash computations.
    ///
    /// # Arguments
    /// * `items` - A vector of (key, value) pairs to insert
    pub fn insert_batch(&mut self, items: Vec<(String, V)>) {
        // OMEGA OPTIMIZATION: Use into_iter to avoid cloning
        for (key, value) in items.into_iter() {
            self.insert(key, value);
        }
    }

        /// Returns statistics about bucket usage.
    ///
    /// # Returns
    /// A tuple containing:
    /// 1. Number of empty buckets
    /// 2. Number of inline buckets
    /// 3. Number of overflow buckets
    pub fn bucket_stats(&self) -> (usize, usize, usize) {
        let mut empty = 0;
        let mut inline = 0; 
        let mut overflow = 0;
        
        for bucket in &self.storage {
            match bucket {
                OmegaBucket::Empty => empty += 1,
                OmegaBucket::Inline { .. } => inline += 1,
                OmegaBucket::Overflow { .. } => overflow += 1,
            }
        }
        
        (empty, inline, overflow)
    }
}

/// Implementation specialized for u64 keys with optimized numeric hashing.
///
/// This specialization provides optimized implementations for u64 keys,
/// using efficient bit manipulation and direct comparisons.
impl<V> OmegaHashSet<u64, V>
where
    V: Clone + Default,
{
    pub fn new_u64_map(table_size: usize) -> Self { Self::new_internal(table_size) }

    #[inline(always)]
    fn get_rank_for_key(&self, key: &u64) -> usize {
        let seed =key.lightning_hash();
        let mut rng = OmegaRng::new(seed);
        let rank = rng.range(0, self.storage.len() as u64 - 1);
        rank as usize
    }

    pub fn insert(&mut self, key: u64, value: V) -> Option<V> {
        let rank_idx = self.get_rank_for_key(&key);
        
        // OMEGA OPTIMIZATION: Direct unsafe access - no bounds checking
        let bucket = unsafe { self.storage.get_unchecked_mut(rank_idx) };
        
        match bucket {
            OmegaBucket::Empty => {
                // OMEGA OPTIMIZATION: Initialize inline bucket with zero allocations
                let mut entries = core::array::from_fn(|_| (u64::default(), V::default()));

                entries[0] = (key, value);
                *bucket = OmegaBucket::Inline { entries, len: 1 };
                self.count += 1;
                None
            }
            OmegaBucket::Inline { entries, len } => {
                // OMEGA OPTIMIZATION: Unrolled search for inline entries
                let current_len = *len as usize;
                
                // Check existing entries for key match
                for i in 0..current_len {
                    let entry = unsafe { entries.get_unchecked_mut(i) };
                    if entry.0 == key {
                        return Some(std::mem::replace(&mut entry.1, value));
                    }
                }
                
                // Add new entry if space available
                if current_len < INLINE_BUCKET_SIZE {
                    entries[current_len] = (key, value);
                    *len += 1;
                    self.count += 1;
                    None
                } else {
                    // OMEGA OPTIMIZATION: Convert to overflow bucket with pre-allocated capacity
                    let mut overflow_vec = Vec::with_capacity(OVERFLOW_BUCKET_SIZE);
                    
                    // Move existing entries
                    for i in 0..INLINE_BUCKET_SIZE {
                        overflow_vec.push(std::mem::take(&mut entries[i]));
                    }
                    overflow_vec.push((key, value));
                    
                    *bucket = OmegaBucket::Overflow { entries: overflow_vec };
                    self.count += 1;
                    None
                }
            }
            OmegaBucket::Overflow { entries } => {
                // OMEGA OPTIMIZATION: Fast search in overflow bucket
                for entry in entries.iter_mut() {
                    if entry.0 == key {
                        return Some(std::mem::replace(&mut entry.1, value));
                    }
                }
                
                // OMEGA OPTIMIZATION: Efficient capacity management
                if entries.len() == entries.capacity() {
                    entries.reserve(entries.capacity());
                }
                entries.push((key, value));
                self.count += 1;
                None
            }
        }
    }

    #[inline(always)]
    pub fn get(&self, key: &u64) -> Option<&V> {
        let rank_idx = self.get_rank_for_key(key);
        
        // OMEGA OPTIMIZATION: Unsafe access for maximum speed
        let bucket = unsafe { self.storage.get_unchecked(rank_idx) };
        
        match bucket {
            OmegaBucket::Empty => None,
            OmegaBucket::Inline { entries, len } => {
                // OMEGA OPTIMIZATION: Unrolled search for inline entries
                let current_len = *len as usize;
                for i in 0..current_len {
                    let (k, v) = unsafe { entries.get_unchecked(i) };
                    if k == key { return Some(v); }
                }
                None
            }
            OmegaBucket::Overflow { entries } => {
                // OMEGA OPTIMIZATION: Efficient search in overflow
                for (k, v) in entries.iter() {
                    if k == key { return Some(v); }
                }
                None
            }
        }
    }


    /// Returns statistics about bucket usage.
    ///
    /// # Returns
    /// A tuple containing:
    /// 1. Number of empty buckets
    /// 2. Number of inline buckets
    /// 3. Number of overflow buckets
    pub fn bucket_stats(&self) -> (usize, usize, usize) {
        let mut empty = 0;
        let mut inline = 0; 
        let mut overflow = 0;
        
        for bucket in &self.storage {
            match bucket {
                OmegaBucket::Empty => empty += 1,
                OmegaBucket::Inline { .. } => inline += 1,
                OmegaBucket::Overflow { .. } => overflow += 1,
            }
        }
        
        (empty, inline, overflow)
    }
}

