# 🚀 vibe-code – Brain-Dead Simple Parallelism for Rust

**For coders who just want things to work fast.**

`vibe-code` is a dead-simple parallel execution engine for Rust that runs your functions on all CPU cores — with **no threads**, **no channels**, and **no async boilerplate**.

Inspired by nature’s circulatory systems — from ants to whales — where the design is identical, only the scale changes. Your code should scale the same way.

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

