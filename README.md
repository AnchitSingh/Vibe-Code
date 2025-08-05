# 🚀 VibeSystem - Brain-Dead Simple Parallelism for Rust

**For coders who just want things to work fast.**

No threads, no channels, no complexity. Just run your functions in parallel and get results back. It's like having a team of super-fast workers who handle all the hard stuff for you.

## ✨ What Can You Do?

- **Run heavy calculations in parallel** - compress files, process images, crunch numbers.
- **Process large batches of data** - without your computer exploding.
- **Make your code ridiculously fast** - with minimal changes.

## 🎯 Dead Simple Examples

### Basic Parallel Work

```rust
use vibe_code::VibeSystem;

let system = VibeSystem::new();

// Run something heavy in the background
let job1 = system.run(compress_file, my_big_file);
let job2 = system.run(process_image, my_photo);
let job3 = system.run(calculate_stuff, my_data);

// Keep doing other things...
println!("Jobs running in background...");

// Get results when you need them
let compressed = job1.get();  // Blocks until done
let processed = job2.get(); 
let result = job3.get();
```

### Batch Processing

```rust
// Process a bunch of stuff at once
let jobs = vec![
    system.run(process_chunk, chunk1),
    system.run(process_chunk, chunk2),
    system.run(process_chunk, chunk3),
    system.run(process_chunk, chunk4),
    system.run(process_chunk, chunk5),
];

// Wait for everything to finish
let results = vibe_code::collect(jobs);
println!("Processed {} items", results.len());
```

## 🎮 Real Examples

### Video Processing
```rust
let system = VibeSystem::new();

// Process video frames in parallel
let frame_jobs: Vec<_> = video_frames
    .into_iter()
    .map(|frame| system.run(apply_filter, frame))
    .collect();

// Get all processed frames
let filtered_frames = vibe_code::collect(frame_jobs);
```

### Web Scraping
```rust
let system = VibeSystem::new();

// Scrape multiple websites at once
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

// Compress multiple files simultaneously
for file in big_files {
    let job = system.run(compress_file, file);
    // Fire and forget - compression happens in background
}
```


```rust
let system = VibeSystem::new();

// A function that takes data
fn process_data(id: i32) -> String {
    thread::sleep(Duration::from_millis(500));
    format!("Processed #{}", id)
}

// --- Sequential (Slow) Way ---
// This would take 10 * 500ms = 5 seconds.
// for i in 0..10 {
//     process_data(i);
// }

// --- VibeSystem (Fast) Way ---
let start_time = Instant::now();

let jobs: Vec<_> = (0..10)
    .map(|i| system.run(process_data, i))
    .collect();

println!("🚀 All 10 jobs submitted instantly.");

// `collect` waits for all jobs to finish and gathers results in order.
let results = collect(jobs);

let duration = start_time.elapsed();
println!("📦 All jobs finished!");
println!("⏱️  Time taken: {:?}. (Much faster than the sequential 5 seconds!)", duration);
```

## 🔧 Setup

Add this to your `Cargo.toml`:
```toml
[dependencies]
vibe_code = "0.1.0"
```

Then, add this to the top of your file:
```rust
use vibe_code::vibe::{VibeSystem, collect};
```

That's it. No configuration needed.

## 📚 API Reference

### VibeSystem
- `VibeSystem::new()` - Creates a new system.
- `system.run(my_func, data)` - Runs a function with input data in parallel.
- `system.go(my_func)` - Runs a function with no input data in parallel.

### Job
- `job.get()` - Waits for the job to finish and returns the result.
- `job.peek()` - Checks if the job is done without waiting. Returns `Some(result)` or `None`.
- `job.is_done()` - Returns `true` if the job is finished.

### Utilities
- `collect(jobs)` - Waits for a `Vec<Job<T>>` to finish and returns a `Vec<T>`.

## 🚨 Error Handling

**There isn't any.** If a function in one of your jobs panics, your whole program will crash with a helpful message:

- `❌ Your job failed! Check your function for bugs.`
- `❌ Job was cancelled - did you shut down the system?`

This is **by design**. Better to crash early with a clear message than to fail silently and leave you wondering what went wrong.

## 🤔 When NOT to Use This

- **Quick scripts** - just use regular code.
- **A single, small operation** - no point in parallelizing one thing.
- **When you need complex error handling** - this library is designed to crash on failure.

## 💡 Philosophy

This library follows the **"vibe coder"** philosophy:

- ✅ **It just works** - no configuration hell.
- ✅ **Fast by default** - handles thousands of tasks per second.
- ✅ **Crash with helpful messages** - better than silent failures.
- ✅ **Zero learning curve** - if you can call a function, you can use this.

You don't need to understand threads, async, channels, or any of that. Just run your code and get results back fast.

## 🎯 Bottom Line

Your slow, sequential code...
```rust
// This...
for item in big_list {
    process(item);  // Slow, one at a time
}
```

Becomes this...
```rust
// Becomes this...
let jobs: Vec<_> = big_list
    .into_iter()
    .map(|item| system.run(process, item))
    .collect();

let results = collect(jobs);  // Fast, all at once
```

**Same logic, way faster. No complexity.** That's the vibe. 🚀

---

*Built with the Vibe System - a biological approach to parallel computing.*
