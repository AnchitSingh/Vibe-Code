# 🚀 vibe-code – Brain-Dead Simple Parallelism for Rust

**For coders who just want things to work fast.**

`vibe-code` is a dead-simple parallel execution engine for Rust that runs your functions on every CPU core — with **zero thread management**, **no channels**, and **no async boilerplate**.

Inspired by nature’s circulatory systems — from ants to whales — where back-pressure is a feature, not a bug, and the design stays identical as it scales. Your code should scale the same way.
---

## ✨ What Can You Do?

- **Run heavy calculations in parallel** – compress files, process images, crunch numbers.
- **Process large batches of data** – without your computer exploding.
- **Make your code ridiculously fast** – with minimal changes.

---

## 🎯 Dead Simple Examples

### Basic Parallel Work

```rust
use vibe_code::VibeSystem;

let system = VibeSystem::new();

let job1 = system.run(compress_file, my_big_file);
let job2 = system.run(process_image, my_photo);
let job3 = system.run(calculate_stuff, my_data);

println!("Jobs running in background...");

let compressed = job1.get();
let processed = job2.get(); 
let result = job3.get();
````

---

### Batch Processing

```rust
let jobs = vec![
    system.run(process_chunk, chunk1),
    system.run(process_chunk, chunk2),
    system.run(process_chunk, chunk3),
    system.run(process_chunk, chunk4),
    system.run(process_chunk, chunk5),
];

let results = vibe_code::collect(jobs);
println!("Processed {} items", results.len());
```

---

## 🎮 Real Examples

### Video Processing

```rust
let system = VibeSystem::new();

let frame_jobs: Vec<_> = video_frames
    .into_iter()
    .map(|frame| system.run(apply_filter, frame))
    .collect();

let filtered_frames = vibe_code::collect(frame_jobs);
```

### Web Scraping

```rust
let system = VibeSystem::new();

let jobs = vec![
    system.run(scrape_website, "https://site1.com"),
    system.run(scrape_website, "https://site2.com"),
    system.run(scrape_website, "https://site3.com"),
];

let scraped_data = vibe_code::collect(jobs);
```

### File Compression

```rust
let system = VibeSystem::new();

for file in big_files {
    let job = system.run(compress_file, file);
    // Fire and forget
}
```

---

### Speed Comparison

```rust
use std::{thread, time::Duration, time::Instant};
use vibe_code::{VibeSystem, collect};

fn process_data(id: i32) -> String {
    thread::sleep(Duration::from_millis(500));
    format!("Processed #{}", id)
}

let system = VibeSystem::new();

let start_time = Instant::now();

let jobs: Vec<_> = (0..10)
    .map(|i| system.run(process_data, i))
    .collect();

println!("🚀 All 10 jobs submitted instantly.");

let results = collect(jobs);

let duration = start_time.elapsed();
println!("📦 All jobs finished!");
println!("⏱️  Time taken: {:?}. (Much faster than the sequential 5 seconds!)", duration);
```

---

## 🔧 Setup

Add to your `Cargo.toml`:

```toml
[dependencies]
vibe-code = "0.1.0"
```

Then use it:

```rust
use vibe_code::{VibeSystem, collect};
```

That's it. No config. No setup hell.

---

## 📚 API Reference

### `VibeSystem`

* `VibeSystem::new()` – Creates a new system.
* `system.run(my_func, data)` – Runs a function with input data in parallel.
* `system.go(my_func)` – Runs a function with no input.

### `Job`

* `job.get()` – Waits for the job to finish and returns the result.
* `job.peek()` – Checks if done without blocking. Returns `Some(result)` or `None`.
* `job.is_done()` – Returns `true` if the job is complete.

### Utilities

* `collect(jobs)` – Waits for `Vec<Job<T>>` to finish and returns `Vec<T>`.

---

## 🚨 Error Handling

This library is intentionally **crash-first**.

If a function in one of your jobs panics, your whole program will crash — loudly — with a helpful message. That’s on purpose.

* `❌ Your job failed! Check your function for bugs.`
* `❌ Job was cancelled - did you shut down the system?`

No silent failures. No mysterious bugs. Fail fast. Fix fast.

---

## 🤔 When *NOT* to Use This

* **For quick scripts** – Just run code normally.
* **For a single small operation** – No point parallelizing one thing.
* **If you need complex error handling** – vibe\_code crashes on failure by design.

---

## 💡 Philosophy

This library follows the **"vibe coder"** philosophy:

* ✅ **It just works** – no setup hell.
* ✅ **Fast by default** – uses all your CPU cores out of the box.
* ✅ **Crash early** – better than bugs hiding in shadows.
* ✅ **Zero learning curve** – if you can call a function, you can use this.

No threads. No channels. No async spaghetti. Just results.

---

## ⚡ Performance Snapshot

Processing 10 jobs (500ms each):

* **Sequential**: \~5 seconds
* **With vibe\_code**: \~0.6 seconds

Tested on 12-core CPU.

---



## 📈 Benchmark: Parallel Matrix Multiplication

Want to see vibe-code in action on a real heavy task? This benchmark multiplies large matrices (e.g., 1000x1100 * 1100x1000) and compares sequential vs. parallel execution. It proves: **bigger loads = bigger wins**!

- **Sequential**: Runs on one core – fine, but slow for big data.
- **Parallel (with vibe-code)**: Splits work across all cores.
- **Expected Speedup**: 3-8x (or more) on multi-core machines. Tune sizes/heavy_factor to match your CPU!

### Example Output (on an 8-core machine)
```
--- 🚀 VibeSystem Demo: Parallel Matrix Multiplication ---

Multiplying 1000x1100 matrix with 1100x1000 matrix (heavy factor: 5)...

🧮 Running sequential (single-threaded) version...

⚡ Running parallel version with VibeSystem...
🚀 Submitted 1000 parallel jobs to VibeSystem...

📊 Results Comparison:
⏱️  Sequential time: 15.636259298s
⏱️  Parallel time:   4.782312656s
🚀 Speedup: 3.3x faster with vibe-code!

Preview of result (top-left 3x3):
31350.0024200.0024750.00
24200.0026400.0022000.00
24750.0022000.0031350.00

--- Demo Complete! Feel the vibe. 🚀 ---
```

### Try It Yourself
Copy this standalone code into a file (e.g., `matrix_bench.rs`), add `vibe-code` to your `Cargo.toml`, and run `cargo run --release`. Adjust sizes for your machine!

```rust
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use vibe_code::{collect, VibeSystem};

/// Represents a simple 2D matrix.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<Vec<f64>>,
}

impl Matrix {
    /// Creates a new matrix filled with zeros.
    pub fn new(rows: usize, cols: usize) -> Self {
        Matrix {
            rows,
            cols,
            data: vec![vec![0.0; cols]; rows],
        }
    }

    /// Creates a matrix with pseudo-random values for demos.
    pub fn random(rows: usize, cols: usize) -> Self {
        let mut matrix = Self::new(rows, cols);
        for i in 0..rows {
            for j in 0..cols {
                // Simple pseudo-random formula (for reproducibility)
                matrix.data[i][j] = (((i + 1) * (j + 1)) as f64) % 10.0;
            }
        }
        matrix
    }
}

/// Custom display for neat matrix printing.
impl fmt::Display for Matrix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in &self.data {
            for &val in row {
                write!(f, "{:8.2}", val)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// Calculates a single row of the result matrix.
///
/// This self-contained function can run sequentially or in parallel.
/// It uses shared access to the right matrix (`b`) via `Arc` for efficiency in parallel mode.
/// To simulate heavier computation, we repeat the inner loop `heavy_factor` times.
fn calculate_row(data: (Vec<f64>, Arc<Matrix>, usize)) -> Vec<f64> {
    let (row_a, matrix_b, heavy_factor) = data;
    let mut result_row = vec![0.0; matrix_b.cols];

    for _ in 0..heavy_factor {  // Repeat to make it "heavier" for demo purposes
        for j in 0..matrix_b.cols {
            let mut sum = 0.0;
            for k in 0..row_a.len() {
                sum += row_a[k] * matrix_b.data[k][j];
            }
            result_row[j] = sum;  // Overwrite – just for compute intensity
        }
    }
    result_row
}

/// Multiplies two matrices sequentially (single-threaded) for comparison.
///
/// # Arguments
/// * `a` - Left matrix.
/// * `b` - Right matrix.
/// * `heavy_factor` - Multiplier to simulate heavier computation.
///
/// # Panics
/// If dimensions are incompatible (`a.cols != b.rows`).
///
/// # Returns
/// The result matrix (`a * b`).
pub fn sequential_multiply(a: &Matrix, b: &Matrix, heavy_factor: usize) -> Matrix {
    if a.cols != b.rows {
        panic!("❌ Incompatible matrix dimensions for multiplication!");
    }

    // Share `b` via Arc (for consistency with parallel version).
    let shared_b = Arc::new(b.clone());

    // Compute each row sequentially.
    let mut result = Matrix::new(a.rows, b.cols);
    for (i, row) in a.data.iter().enumerate() {
        result.data[i] = calculate_row((row.clone(), shared_b.clone(), heavy_factor));
    }
    result
}

/// Multiplies two matrices in parallel using `VibeSystem`.
///
/// # Arguments
/// * `a` - Left matrix.
/// * `b` - Right matrix.
/// * `heavy_factor` - Multiplier to simulate heavier computation.
///
/// # Panics
/// If dimensions are incompatible (`a.cols != b.rows`).
///
/// # Returns
/// The result matrix (`a * b`).
pub fn parallel_multiply(a: &Matrix, b: &Matrix, heavy_factor: usize) -> Matrix {
    if a.cols != b.rows {
        panic!("❌ Incompatible matrix dimensions for multiplication!");
    }

    // 1. Initialize the system – it handles all parallelism behind the scenes.
    let system = VibeSystem::new();

    // Share `b` efficiently across jobs (cheap clones via Arc).
    let shared_b = Arc::new(b.clone());

    // 2. Submit a job per row – runs in background instantly.
    let jobs: Vec<_> = a.data
        .iter()
        .map(|row| {
            system.run(
                calculate_row,
                (row.clone(), shared_b.clone(), heavy_factor),  // Clone row (small) and Arc (cheap)
            )
        })
        .collect();

    println!("🚀 Submitted {} parallel jobs to VibeSystem...", jobs.len());

    // 3. Collect results – waits for all jobs and preserves order.
    let result_rows = collect(jobs);

    // 4. Build the final matrix.
    let mut result = Matrix::new(a.rows, b.cols);
    result.data = result_rows;
    result
}

/// Demo: Compares sequential vs. parallel matrix multiplication.
fn main() {
    println!("--- 🚀 VibeSystem Demo: Parallel Matrix Multiplication ---");

    // Tune these for your machine! Larger = more speedup, but watch RAM/ time.
    let rows_a = 1000;
    let cols_a = 1100;
    let rows_b = cols_a;  // Must match for multiplication
    let cols_b = 1000;
    let heavy_factor = 5;  // Increase for "heavier" computation (simulates real workloads)

    // Create matrices.
    let matrix_a = Matrix::random(rows_a, cols_a);
    let matrix_b = Matrix::random(rows_b, cols_b);

    println!(
        "\nMultiplying {}x{} matrix with {}x{} matrix (heavy factor: {})...",
        matrix_a.rows, matrix_a.cols, matrix_b.rows, matrix_b.cols, heavy_factor
    );

    // --- Sequential Run ---
    println!("\n🧮 Running sequential (single-threaded) version...");
    let seq_start = Instant::now();
    let seq_result = sequential_multiply(&matrix_a, &matrix_b, heavy_factor);
    let seq_duration = seq_start.elapsed();

    // --- Parallel Run ---
    println!("\n⚡ Running parallel version with VibeSystem...");
    let par_start = Instant::now();
    let par_result = parallel_multiply(&matrix_a, &matrix_b, heavy_factor);
    let par_duration = par_start.elapsed();

    // --- Comparison ---
    println!("\n📊 Results Comparison:");
    println!("⏱️  Sequential time: {:?}", seq_duration);
    println!("⏱️  Parallel time:   {:?}", par_duration);

    let speedup = seq_duration.as_secs_f64() / par_duration.as_secs_f64();
    println!("🚀 Speedup: {:.1}x faster with vibe-code!", speedup);
    println!("(Tip: Increase matrix sizes or heavy_factor for bigger wins. Run in --release mode!)");

    // Verify results match (for demo integrity).
    assert_eq!(seq_result, par_result);

    // Optional: Print a small preview (first 3x3) to avoid flooding output.
    println!("\nPreview of result (top-left 3x3):");
    for i in 0..3.min(par_result.rows) {
        for j in 0..3.min(par_result.cols) {
            print!("{:8.2}", par_result.data[i][j]);
        }
        println!();
    }

    println!("\n--- Demo Complete! Feel the vibe. 🚀 ---");
}
```

---

## 🎯 Bottom Line

Your slow, sequential code:

```rust
for item in big_list {
    process(item);  // Slow, one at a time
}
```

Becomes this:

```rust
let jobs: Vec<_> = big_list
    .into_iter()
    .map(|item| system.run(process, item))
    .collect();

let results = collect(jobs);  // Fast, all at once
```

**Same logic. Way faster. Zero complexity.** That’s the vibe. 🚀

---

*Inspired by biology: ants and whales use the same circulatory system — just scaled. vibe\_code works the same way.*

