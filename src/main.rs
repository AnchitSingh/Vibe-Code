//! # VibeSystem Demo
//!
//! This file demonstrates how to use the `VibeSystem` for simple, powerful parallelism.
//! To see it in action, run `cargo run --release`.
//! The `--release` flag is recommended to see the true performance benefits.

use std::thread;
use std::time::{Duration, Instant};
use vibe_code::vibe::{VibeSystem, collect};

/// A simple function that simulates a long-running task.
fn do_some_heavy_work() -> String {
    println!("⚙️  Starting some heavy work...");
    thread::sleep(Duration::from_secs(1));
    println!("✅ Heavy work done!");
    "Work complete".to_string()
}

/// A function that simulates processing a piece of data.
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
    // Initialize the timer for internal stats (optional).
    println!("--- Welcome to the VibeSystem Demo ---");

    // Create a new VibeSystem. This sets up the background worker pool
    // and load balancer, ready to run jobs in parallel.
    let system = VibeSystem::new();

    // --- Example 1: Run a function with no arguments using `.go()` ---
    println!(
        "
--- Example 1: Running a simple background job with .go() ---"
    );
    let job1 = system.go(do_some_heavy_work);
    println!("🚀 Job 1 submitted! The code continues to run without waiting.");

    // `.get()` waits for the job to finish and returns its result.
    let result1 = job1.get();
    println!("📦 Got result from Job 1: '{result1}'");

    // --- Example 2: Run a function with arguments using `.run()` ---
    println!(
        "
--- Example 2: Running a function with data using .run() ---"
    );
    let job2 = system.run(process_data, (101, "alpha"));
    println!("🚀 Job 2 submitted! Let's get the result.");
    let result2 = job2.get();
    println!("📦 Got result from Job 2: '{result2}'");

    // --- Example 3: True Parallelism ---
    // Run 10 jobs that each take 500ms.
    // Sequentially, this would take 5 seconds. With VibeSystem, it's much faster.
    println!(
        "
--- Example 3: Running 10 jobs in parallel ---"
    );
    let start_time = Instant::now();

    let mut jobs = Vec::new();
    for i in 0..10 {
        // `.run()` submits the job and immediately returns, allowing the loop
        // to continue without waiting.
        let user_data = (i, "user");
        let job = system.run(process_data, user_data);
        jobs.push(job);
    }
    println!("🚀 All 10 jobs submitted instantly.");

    // `collect` is a helper that waits for all jobs in a vector to finish
    // and returns a vector of their results, in order.
    let results = collect(jobs);

    let duration = start_time.elapsed();
    println!(
        "
📦 All 10 jobs finished!"
    );
    println!("Results: {results:?}");
    println!("⏱️  Time taken: {duration:?}. (Much faster than the sequential 5 seconds!)");

    println!(
        "
--- Demo Complete ---"
    );
}
