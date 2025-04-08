use std::time::Instant;
use std::process::Command;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // First, check if the large trace file exists, if not generate it
    let trace_file = "large_trace.ftf";
    
    if !std::path::Path::new(trace_file).exists() {
        println!("Large trace file not found. Generating it first...");
        Command::new("cargo")
            .args(["run", "--release", "--example", "generate_large_trace"])
            .status()?;
    }
    
    let file_info = fs::metadata(trace_file)?;
    let input_size_mb = file_info.len() as f64 / (1024.0 * 1024.0);
    println!("Input file size: {:.2} MB", input_size_mb);
    
    // Define different time ranges to test
    let test_ranges = [
        // Start timestamp, end timestamp, description
        (1000, 1000000, "Small Range (0.001%)"),
        (1000, 10000000, "Medium Range (10%)"),
        (1000, 25000000, "Half Range (50%)"),
        (1000, 40000000, "Large Range (80%)"),
        (2500000, 3500000, "Middle 1M Range (2%)"),
    ];
    
    // Run benchmarks for each range
    for (start_ts, end_ts, description) in test_ranges {
        let output_file = format!("large_trace_cut_{}_to_{}.ftf", start_ts, end_ts);
        
        println!("\nRunning benchmark for {} ({} to {})", description, start_ts, end_ts);
        let start_time = Instant::now();
        
        let status = Command::new("cargo")
            .args([
                "run", 
                "--release", 
                "--", 
                "--start-ts", 
                &start_ts.to_string(), 
                "--end-ts", 
                &end_ts.to_string(),
                "--input-path",
                trace_file,
                "--output-path",
                &output_file,
            ])
            .status()?;
        
        if status.success() {
            let duration = start_time.elapsed();
            println!("Processing completed in {:.2} seconds", duration.as_secs_f64());
            
            if let Ok(output_info) = fs::metadata(&output_file) {
                let output_size_mb = output_info.len() as f64 / (1024.0 * 1024.0);
                let reduction_pct = (1.0 - (output_size_mb / input_size_mb)) * 100.0;
                
                println!("Output file size: {:.2} MB", output_size_mb);
                println!("Size reduction: {:.2}%", reduction_pct);
                println!("Processing speed: {:.2} MB/s", input_size_mb / duration.as_secs_f64());
                
                // Calculate events per second (rough estimate)
                let approx_events_processed = 50_000_000.0; // Total events in the input file
                let events_per_second = approx_events_processed / duration.as_secs_f64();
                println!("Approximate processing speed: {:.2} events/second", events_per_second);
            } else {
                println!("Could not get output file info");
            }
        } else {
            println!("Failed to process trace file");
        }
    }
    
    Ok(())
}