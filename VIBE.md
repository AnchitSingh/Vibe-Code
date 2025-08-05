# 🚀 Vibe - The Brain-Dead Simple Parallel System

**For normal humans who just want things to work fast.**

No threads, no channels, no complexity. Just run your stuff in parallel and get results back. It's like having a team of super-fast workers who handle everything for you.

## ✨ What Can You Do?

- **Run heavy calculations in parallel** - compress files, process images, crunch numbers
- **Send network messages** - communicate between drones, servers, IoT devices  
- **Handle thousands of tasks** - without your computer exploding

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

### Fire-and-Forget Messages

```rust
// Create a network connection
let socket = system.ovp_socket("eth0", my_device_id);

// Send messages (doesn't wait for response)
socket.send_to(vec![device1, device2, device3], b"hello friends");
socket.broadcast(b"hello everyone");

// Check for incoming messages
if let Some(message) = socket.try_message() {
    println!("Got: {:?}", message);
}
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

### Drone Communication
```rust
let system = VibeSystem::new();
let socket = system.ovp_socket("wlan0", my_drone_id);

// Send telemetry to base station
socket.send_to(vec![base_station_id], &telemetry_data);

// Broadcast emergency signal
socket.broadcast(b"EMERGENCY: LOW BATTERY");

// Listen for commands
loop {
    if let Some(command) = socket.try_message() {
        let job = system.run(execute_command, command);
        // Command executes in background
    }
    
    // Keep flying...
    std::thread::sleep(Duration::from_millis(100));
}
```

## 🔧 Setup

Add to your `Cargo.toml`:
```toml
[dependencies]
vibe_code = { path = "../path/to/vibe_code" }
```

That's it. No configuration needed.

## 📚 API Reference

### VibeSystem
- `VibeSystem::new()` - Create a new system
- `system.run(function, data)` - Run something in parallel
- `system.go(|| code)` - Run code with no input data
- `system.ovp_socket(interface, id)` - Create network socket

### Job
- `job.get()` - Wait for result (blocks)
- `job.peek()` - Check if done (doesn't block)
- `job.is_done()` - Returns true/false

### OvpSocket  
- `socket.send_to(targets, data)` - Send to specific devices
- `socket.broadcast(data)` - Send to everyone
- `socket.next_message()` - Wait for message (blocks)
- `socket.try_message()` - Check for message (doesn't block)

### Utilities
- `vibe_code::collect(jobs)` - Wait for all jobs to finish

## 🚨 Error Handling

**There isn't any.** If something breaks, your program will crash with a helpful message like:

- `❌ Your job failed! Check your function for bugs.`
- `💥 Failed to send message - system overloaded!`
- `📡 Network error while receiving message`

This is **by design**. Better to crash early with a clear message than silently fail.

## ⚡ Performance Tips

**For normal workloads:** Just use `VibeSystem::new()` - it works great.

**For heavy workloads:** Scale up the system:
```rust
let system = VibeSystem::builder()
    .with_nodes(80)        // More parallel workers
    .with_super_nodes(40)  // More high-priority workers  
    .build();
```

**Rule of thumb:** 
- Light work (few hundred operations): default settings
- Heavy work (thousands of operations): scale up nodes
- Extreme work (tens of thousands): scale up more

## 🤔 When NOT to Use This

- **Quick scripts** - just use regular code
- **Single operations** - no point in parallelizing one thing
- **Memory-critical applications** - this uses more memory for speed

## 💡 Philosophy

This library follows the **"vibe coder"** philosophy:

- ✅ **It just works** - no configuration hell
- ✅ **Fast by default** - handles 40,000+ operations per second
- ✅ **Crash with helpful messages** - better than silent failures  
- ✅ **Zero learning curve** - if you can call a function, you can use this

You don't need to understand threads, async, channels, or any complex stuff. Just run your code and get results back fast.

## 🎯 Bottom Line

```rust
// This...
for item in big_list {
    process(item);  // Slow, one at a time
}

// Becomes this...
let jobs: Vec<_> = big_list
    .into_iter()
    .map(|item| system.run(process, item))
    .collect();
let results = vibe_code::collect(jobs);  // Fast, all at once
```

**Same code, way faster. No complexity.** That's the vibe. 🚀

---

*Built with the Ultra Omega Vibe System - a biological approach to parallel computing.*