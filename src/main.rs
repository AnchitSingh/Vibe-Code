// Welcome to the vibe-system - The dead-simple way to run stuff in parallel.
// No complex setup, no manual thread management. Just vibe and code.
//
// This file shows you how it works with a few examples.
// To run this, just use: `cargo run --release`
// (Use --release for a real performance feel!)

use std::thread;
use std::time::{Duration, Instant};
use vibe_code::utils::timer_init;
use vibe_code::vibe::{VibeSystem, collect};
/// This is a simple function we want to run in the background.
/// It pretends to do some heavy work by sleeping for a second.
fn do_some_heavy_work() -> String {
    println!("⚙️  Starting some heavy work...");
    thread::sleep(Duration::from_secs(1));
    println!("✅ Heavy work done!");
    "Work complete".to_string()
}

/// This function takes some data, processes it, and returns a result.
/// It also pretends to be slow.
fn process_data(data: (i32, &str)) -> String {
    let (id, name) = data;
    println!("⚙️  Processing data for id: {id}, name: '{name}'...");
    thread::sleep(Duration::from_millis(500));
    println!("✅ Finished processing for id: {id}");
    format!(
        "Processed {id}: {name}",
        id = id,
        name = name.to_uppercase()
    )
}

fn main() {
    let _ = timer_init();
    println!("--- Welcome to the vibe-system Demo ---");

    // 1. Create a new VibeSystem.
    // This one-liner sets up a powerful, load-balanced, multi-threaded engine.
    // It's ready to run your code in parallel across your CPU cores.
    let system = VibeSystem::new();

    // --- Example 1: Running a simple function with `go` ---
    // `go` is for when you have a function that doesn't need any input.
    println!("--- Example 1: Running a simple background job with .go() ---");
    let job1 = system.go(do_some_heavy_work);
    println!("🚀 Job 1 submitted! The code continues to run without waiting.\n");

    // We can do other things here while the job runs in the background...
    thread::sleep(Duration::from_millis(100));

    // Now, let's get the result. `.get()` will wait until the job is done.
    let result1 = job1.get();
    println!("📦 Got result from Job 1: '{result1}'\n");

    // --- Example 2: Running a function with data using `run` ---
    // `run` is for when you need to pass data to your function.
    println!("--- Example 2: Running a function with data using .run() ---");
    let job2 = system.run(process_data, (101, "alpha"));
    println!("🚀 Job 2 submitted! Let's get the result.\n");
    let result2 = job2.get();
    println!("📦 Got result from Job 2: '{result2}'\n");

    // --- Example 3: True Parallelism - The real power of VibeSystem ---
    // Let's run 10 jobs that each take 500ms.
    // Sequentially, this would take 10 * 500ms = 5 seconds.
    // With VibeSystem, it should be much faster!
    println!("--- Example 3: Running 10 jobs in parallel ---");
    let start_time = Instant::now();

    let mut jobs = Vec::new();
    for i in 0..10 {
        // Each call to `run` fires off a job into the background immediately.
        let user_data = (i, "user");
        let job = system.run(process_data, user_data);
        jobs.push(job);
    }
    println!("🚀 All 10 jobs submitted instantly.");

    // The `collect` function is a handy utility that waits for all jobs
    // in the vector to finish and collects their results in order.
    let results = collect(jobs);

    let duration = start_time.elapsed();
    println!("\n📦 All 10 jobs finished!");
    println!("Results: {results:?}");
    println!("⏱️  Time taken: {duration:?}. (Much faster than the sequential 5 seconds!)");

    println!("\n--- Demo Complete ---");
}
