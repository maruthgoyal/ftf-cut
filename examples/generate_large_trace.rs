use ftfrs::{Archive, Record, StringRef, ThreadRef, Argument};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration
    const NUM_EVENTS: usize = 50_000_000;
    const NEW_STRING_INTERVAL: usize = 2_000_000;
    const OUTPUT_FILE: &str = "large_trace.ftf";
    
    println!("Generating trace with {} events...", NUM_EVENTS);
    let start_time = Instant::now();
    
    // Create output file
    let file = File::create(OUTPUT_FILE)?;
    let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file); // 8MB buffer
    
    // Create initial archive structure with essential records
    let mut archive = Archive {
        records: Vec::new(),
    };
    
    // Add magic number record
    archive.records.push(Record::create_magic_number());
    
    // Add initialization record with ticks per second
    archive.records.push(Record::create_initialization(1_000_000_000)); // 1 billion ticks per second
    
    // Add base string records
    archive.records.push(Record::create_string(1, "category".to_string()));
    archive.records.push(Record::create_string(2, "event".to_string()));
    archive.records.push(Record::create_string(3, "arg_name".to_string()));
    archive.records.push(Record::create_string(4, "arg_value".to_string()));
    
    // Write the header records
    archive.write(&mut writer)?;
    
    // Clear the archive to avoid storing too much in memory
    archive.records.clear();
    
    // Set up variables for event generation
    let mut current_timestamp = 1000;
    let mut next_string_id = 5;
    
    // Track progress
    let progress_interval = NUM_EVENTS / 100;
    let mut last_progress = 0;
    
    // Generate events
    for i in 0..NUM_EVENTS {
        // Add new string records periodically
        if i > 0 && i % NEW_STRING_INTERVAL == 0 {
            let category_id = next_string_id;
            let category_name = format!("category_{}", i / NEW_STRING_INTERVAL);
            archive.records.push(Record::create_string(category_id, category_name));
            
            let event_id = next_string_id + 1;
            let event_name = format!("event_{}", i / NEW_STRING_INTERVAL);
            archive.records.push(Record::create_string(event_id, event_name));
            
            let arg_name_id = next_string_id + 2;
            let arg_name = format!("arg_name_{}", i / NEW_STRING_INTERVAL);
            archive.records.push(Record::create_string(arg_name_id, arg_name));
            
            let arg_value_id = next_string_id + 3;
            let arg_value = format!("arg_value_{}", i / NEW_STRING_INTERVAL);
            archive.records.push(Record::create_string(arg_value_id, arg_value));
            
            next_string_id += 4;
        }
        
        // Choose string references based on the current batch
        let string_batch = i / NEW_STRING_INTERVAL;
        let category_id = if string_batch == 0 { 1 } else { 5 + (string_batch - 1) * 4 };
        let event_id = if string_batch == 0 { 2 } else { 6 + (string_batch - 1) * 4 };
        let arg_name_id = if string_batch == 0 { 3 } else { 7 + (string_batch - 1) * 4 };
        let arg_value_id = if string_batch == 0 { 4 } else { 8 + (string_batch - 1) * 4 };
        
        // Create event with increasing timestamp
        let event_type = i % 5;
        
        match event_type {
            0 => {
                // Duration Begin
                archive.records.push(Record::create_duration_begin_event(
                    current_timestamp,
                    ThreadRef::Inline { process_koid: 100, thread_koid: 200 },
                    StringRef::Ref(category_id as u16),
                    StringRef::Ref(event_id as u16),
                    vec![Argument::Str(StringRef::Ref(arg_name_id as u16), StringRef::Ref(arg_value_id as u16))],
                ));
            },
            1 => {
                // Duration End
                archive.records.push(Record::create_duration_end_event(
                    current_timestamp,
                    ThreadRef::Inline { process_koid: 100, thread_koid: 200 },
                    StringRef::Ref(category_id as u16),
                    StringRef::Ref(event_id as u16),
                    vec![Argument::Str(StringRef::Ref(arg_name_id as u16), StringRef::Ref(arg_value_id as u16))],
                ));
            },
            2 => {
                // Instant
                archive.records.push(Record::create_instant_event(
                    current_timestamp,
                    ThreadRef::Inline { process_koid: 100, thread_koid: 200 },
                    StringRef::Ref(category_id as u16),
                    StringRef::Ref(event_id as u16),
                    vec![Argument::Str(StringRef::Ref(arg_name_id as u16), StringRef::Ref(arg_value_id as u16))],
                ));
            },
            3 => {
                // Counter
                archive.records.push(Record::create_counter_event(
                    current_timestamp,
                    ThreadRef::Inline { process_koid: 100, thread_koid: 200 },
                    StringRef::Ref(category_id as u16),
                    StringRef::Ref(event_id as u16),
                    vec![Argument::Str(StringRef::Ref(arg_name_id as u16), StringRef::Ref(arg_value_id as u16))],
                    i as u64 % 1000, // counter_id
                ));
            },
            4 => {
                // Duration Complete
                archive.records.push(Record::create_duration_complete_event(
                    current_timestamp,
                    ThreadRef::Inline { process_koid: 100, thread_koid: 200 },
                    StringRef::Ref(category_id as u16),
                    StringRef::Ref(event_id as u16),
                    vec![Argument::Str(StringRef::Ref(arg_name_id as u16), StringRef::Ref(arg_value_id as u16))],
                    current_timestamp + 100, // end_ts
                ));
            },
            _ => unreachable!(),
        }
        
        // Increment timestamp - make it have some variation
        current_timestamp += 100 + (i as u64 % 10);
        
        // Write and clear records batch periodically to avoid excessive memory usage
        if archive.records.len() >= 10000 {
            archive.write(&mut writer)?;
            archive.records.clear();
        }
        
        // Show progress
        if i - last_progress >= progress_interval {
            last_progress = i;
            let percent = (i * 100) / NUM_EVENTS;
            println!("Progress: {}% ({}/{} events)", percent, i, NUM_EVENTS);
        }
    }
    
    // Write any remaining records
    if !archive.records.is_empty() {
        archive.write(&mut writer)?;
    }
    
    // Flush writer and get file info
    writer.flush()?;
    drop(writer); // Close the file
    
    // Get file size
    let file_info = std::fs::metadata(OUTPUT_FILE)?;
    let file_size_mb = file_info.len() as f64 / (1024.0 * 1024.0);
    
    let duration = start_time.elapsed();
    println!("Generation completed in {:.2} seconds", duration.as_secs_f64());
    println!("Generated file: {}", OUTPUT_FILE);
    println!("File size: {:.2} MB", file_size_mb);
    println!("Events: {}", NUM_EVENTS);
    println!("Bytes per event: {:.2}", file_info.len() as f64 / NUM_EVENTS as f64);
    println!("String records: {}", next_string_id - 1);
    
    Ok(())
}