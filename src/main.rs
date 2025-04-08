use anyhow::{Ok, Result, anyhow};
use ftfrs::{Event, EventRecord, Record, RecordHeader, RecordType, StringRecord, StringRef};
use std::{
    fs::File,
    io::{BufReader, BufWriter, ErrorKind, Read, Seek, Write},
    path::PathBuf,
};

use rustc_hash::{FxHashMap, FxHashSet};

use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    start_ts: u64,
    #[arg(short, long)]
    end_ts: u64,
    #[arg(short, long, value_name = "FILE")]
    input_path: PathBuf,
    #[arg(short, long, value_name = "FILE")]
    output_path: PathBuf,
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    let input = BufReader::new(File::open(cli.input_path)?);
    let output = BufWriter::new(File::create(cli.output_path)?);
    let mut cutter = Cutter::new(input, output, cli.start_ts, cli.end_ts);
    println!("Cutting");
    cutter.cut()?;
    println!("Done");
    Ok(())
}

struct Cutter<R: Read + Seek, W: Write> {
    input: R,
    output: W,
    index_to_offset: FxHashMap<u16, u64>,
    written_indexes: FxHashSet<u16>,
    start_ts: u64,
    end_ts: u64,
}

impl<R: Read + Seek, W: Write> Cutter<R, W> {
    fn new(input: R, output: W, start_ts: u64, end_ts: u64) -> Self {
        let index_to_offset = FxHashMap::default();
        let written_indexes = FxHashSet::default();
        Self {
            input,
            output,
            index_to_offset,
            written_indexes,
            start_ts,
            end_ts,
        }
    }

    fn cut(&mut self) -> Result<()> {
        let mut header_buf = [0_u8; 8];

        loop {
            let pos = self.input.stream_position()?;
            if let Err(e) = self.input.read_exact(&mut header_buf) {
                if e.kind() == ErrorKind::UnexpectedEof {
                    break;
                }
            }

            let header = RecordHeader {
                value: u64::from_ne_bytes(header_buf),
            };
            let record_type = header.record_type()?;
            match record_type {
                RecordType::String => {
                    let index = StringRecord::index_from_header(&header);
                    self.index_to_offset.insert(index, pos);
                    let jump = ((header.size() - 1) as u32) * 8;
                    self.input.seek_relative(jump.into())?;
                }
                RecordType::Event => {
                    self.input.seek_relative(-8)?;
                    let event = Record::from_bytes(&mut self.input)?;
                    if let Record::Event(e) = &event {
                        let write_it = match e {
                            EventRecord::DurationBegin(d) => self.process_event(d.event())?,
                            EventRecord::DurationEnd(d) => self.process_event(d.event())?,
                            EventRecord::DurationComplete(d) => self.process_event(d.event())?,
                            EventRecord::Counter(c) => self.process_event(c.event())?,
                            EventRecord::Instant(i) => self.process_event(i.event())?,
                            _ => true,
                        };

                        if write_it { 
                            event.write(&mut self.output)?;
                        }
                    }
                }
                _ => {
                    self.output.write_all(&header_buf)?;
                    if header.size() > 1 {
                        let mut rest = vec![0_u8; (header.size() as usize - 1) * 8];
                        self.input.read_exact(&mut rest)?;
                        self.output.write_all(&rest)?;
                    }
                }
            }
            // break;
        }
        Ok(())
    }

    fn maybe_write_str_ref(&mut self, idx: u16) -> Result<()> {
        if self.written_indexes.contains(&idx) {
            return Ok(());
        }
        if let Some(offset) = self.index_to_offset.get(&idx) {
            let pos = self.input.stream_position()?;
            // self.input.seek(std::io::SeekFrom::Start(*offset))?;
            self.input.seek_relative(-((pos - *offset) as i64))?;

            let mut header_buf = [0_u8; 8];
            self.input.read_exact(&mut header_buf)?;

            let header = RecordHeader {
                value: u64::from_ne_bytes(header_buf),
            };
            self.output.write_all(&header_buf)?;
            if header.size() > 1 {
                let mut rest = vec![0_u8; (header.size() as usize - 1) * 8];
                self.input.read_exact(&mut rest)?;
                self.output.write_all(&rest)?;
            }

            let jump = pos -  (*offset + (header.size() * 8) as u64);
            self.input.seek_relative(jump as i64)?;
        } else {
            return Err(anyhow!("Referenced String index missing: {idx}"));
        }
        Ok(())
    }

    fn process_event(&mut self, event: &Event) -> Result<bool> {
        let ts = event.timestamp();
        if ts < self.start_ts || ts > self.end_ts {
            return Ok(false);
        }
        if let StringRef::Ref(idx) = event.name() {
            self.maybe_write_str_ref(*idx)?
        }

        if let StringRef::Ref(idx) = event.category() {
            self.maybe_write_str_ref(*idx)?
        }

        for arg in event.arguments() {
            let name_ref = arg.name();
            if let StringRef::Ref(idx) = name_ref {
                self.maybe_write_str_ref(*idx)?
            }
            if let ftfrs::Argument::Str(_, StringRef::Ref(idx)) = arg {
                self.maybe_write_str_ref(*idx)?
            }
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use ftfrs::{Argument, ThreadRef};

    // Helper to create test FTF data
    fn create_test_data() -> Vec<u8> {
        let mut buffer = Vec::new();
        
        // Create some string records
        let event_name = "test_event".to_string();
        let category = "test_category".to_string();
        let arg_name = "arg_key".to_string();
        let arg_value = "arg_value".to_string();
        
        // Write string records
        Record::create_string(1, event_name.clone()).write(&mut buffer).unwrap();
        Record::create_string(2, category.clone()).write(&mut buffer).unwrap();
        Record::create_string(3, arg_name.clone()).write(&mut buffer).unwrap();
        Record::create_string(4, arg_value.clone()).write(&mut buffer).unwrap();
        
        // Create event records with different timestamps
        // Event at timestamp 100 (before range)
        Record::create_duration_begin_event(
            100, 
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Ref(2), 
            StringRef::Ref(1),
            vec![Argument::Str(StringRef::Ref(3), StringRef::Ref(4))],
        ).write(&mut buffer).unwrap();
        
        // Event at timestamp 1000 (in range)
        Record::create_duration_end_event(
            1000, 
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Ref(2), 
            StringRef::Ref(1),
            vec![Argument::Str(StringRef::Ref(3), StringRef::Ref(4))],
        ).write(&mut buffer).unwrap();
        
        // Event at timestamp 2000 (in range)
        Record::create_instant_event(
            2000, 
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Ref(2), 
            StringRef::Ref(1),
            vec![Argument::Str(StringRef::Ref(3), StringRef::Ref(4))],
        ).write(&mut buffer).unwrap();
        
        // Event at timestamp 3000 (after range)
        Record::create_counter_event(
            3000, 
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Ref(2), 
            StringRef::Ref(1),
            vec![Argument::Str(StringRef::Ref(3), StringRef::Ref(4))],
            0, // counter_id
        ).write(&mut buffer).unwrap();
        
        // Event at timestamp 1500 (in range)
        Record::create_duration_complete_event(
            1500, 
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Ref(2), 
            StringRef::Ref(1),
            vec![Argument::Str(StringRef::Ref(3), StringRef::Ref(4))],
            1600, // end_ts
        ).write(&mut buffer).unwrap();
        
        buffer
    }

    // Helper to count events in a buffer within a specific timestamp range
    fn count_events_in_buffer(buffer: &[u8], start_ts: u64, end_ts: u64) -> usize {
        let reader = Cursor::new(buffer);
        let archive = ftfrs::Archive::read(reader).unwrap();
        let mut count = 0;
        
        for record in &archive.records {
            if let Record::Event(event_record) = record {
                let ts = match &event_record {
                    EventRecord::DurationBegin(d) => d.event().timestamp(),
                    EventRecord::DurationEnd(d) => d.event().timestamp(),
                    EventRecord::DurationComplete(d) => d.event().timestamp(),
                    EventRecord::Counter(c) => c.event().timestamp(),
                    EventRecord::Instant(i) => i.event().timestamp(),
                    _ => 0,
                };
                
                if ts >= start_ts && ts <= end_ts {
                    count += 1;
                }
            }
        }
        
        count
    }
    
    // Helper to count string records and collect their indices
    fn count_string_records(buffer: &[u8]) -> (usize, Vec<u16>) {
        let reader = Cursor::new(buffer);
        let archive = ftfrs::Archive::read(reader).unwrap();
        let mut count = 0;
        let mut indices = Vec::new();
        
        for record in &archive.records {
            if let Record::String(string_rec) = record {
                count += 1;
                indices.push(string_rec.index());
            }
        }
        
        (count, indices)
    }

    #[test]
    fn test_cutter_filters_by_timestamp() {
        // Create test data
        let input_data = create_test_data();
        let input_reader = Cursor::new(input_data.clone());
        let mut output_buffer = Vec::new();
        let output_writer = Cursor::new(&mut output_buffer);
        
        // Define time range to include events at 1000, 1500, and 2000
        let start_ts = 500;
        let end_ts = 2500;
        
        // Create cutter and process
        let mut cutter = Cutter::new(input_reader, output_writer, start_ts, end_ts);
        cutter.cut().unwrap();
        
        // Verify: input has 5 events, output should have 3 events in the time range
        let event_count_input = count_events_in_buffer(&input_data, 0, u64::MAX);
        assert_eq!(event_count_input, 5, "Input should have 5 events");
        
        let event_count_output = count_events_in_buffer(&output_buffer, 0, u64::MAX);
        assert_eq!(event_count_output, 3, "Output should have 3 events after filtering");
        
        // Check that only events in the time range were included
        let events_in_range = count_events_in_buffer(&output_buffer, start_ts, end_ts);
        assert_eq!(events_in_range, 3, "All output events should be within the specified time range");
    }

    #[test]
    fn test_string_references_preserved() {
        // Create test data
        let input_data = create_test_data();
        let input_reader = Cursor::new(input_data);
        let mut output_buffer = Vec::new();
        let output_writer = Cursor::new(&mut output_buffer);
        
        // Define time range to include only one event (the Duration End at ts=1000)
        let start_ts = 1000;
        let end_ts = 1000;
        
        // Create cutter and process
        let mut cutter = Cutter::new(input_reader, output_writer, start_ts, end_ts);
        cutter.cut().unwrap();
        
        // Read the output buffer and verify it contains string records
        let (string_record_count, _) = count_string_records(&output_buffer);
        
        // All the strings should be included because they're referenced by the event at ts=1000
        assert_eq!(string_record_count, 4, "Output should contain string records referenced by events");
    }
    
    #[test]
    fn test_unnecessary_strings_not_included() {
        // Create extended test data with additional strings and events
        let mut buffer = create_test_data();
        
        // Add an extra string that will only be referenced by the event at ts=3000 (outside range)
        Record::create_string(5, "unused_in_range".to_string()).write(&mut buffer).unwrap();
        
        // Add an event at ts=3000 that references the new string
        Record::create_counter_event(
            3000, 
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Ref(2), 
            StringRef::Ref(5),  // Reference to the unused string
            vec![Argument::Str(StringRef::Ref(3), StringRef::Ref(4))],
            1, // counter_id
        ).write(&mut buffer).unwrap();
        
        let input_reader = Cursor::new(buffer);
        let mut output_buffer = Vec::new();
        let output_writer = Cursor::new(&mut output_buffer);
        
        // Define time range to exclude the event at ts=3000
        let start_ts = 500;
        let end_ts = 2500;
        
        // Create cutter and process
        let mut cutter = Cutter::new(input_reader, output_writer, start_ts, end_ts);
        cutter.cut().unwrap();
        
        // Read the output buffer and check which string indices are included
        let (_, string_indices) = count_string_records(&output_buffer);
        
        // Verify that string index 5 is not included, as it's only referenced by the excluded event
        assert!(!string_indices.contains(&5), "Output should not contain unnecessary string records");
        
        // Verify that the necessary strings (indices 1-4) are included
        assert!(string_indices.contains(&1), "Output missing required string with index 1");
        assert!(string_indices.contains(&2), "Output missing required string with index 2");
        assert!(string_indices.contains(&3), "Output missing required string with index 3");
        assert!(string_indices.contains(&4), "Output missing required string with index 4");
    }

    #[test]
    fn test_process_event_within_range() {
        // Create a test event within range
        let event = Event::new(
            1500, // timestamp within range
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Inline("test_cat".to_string()),
            StringRef::Inline("test".to_string()),
            Vec::new(),
        );
        
        let mut input_buffer = Vec::new();
        let input = Cursor::new(&mut input_buffer);
        let mut output_buffer = Vec::new();
        let output = Cursor::new(&mut output_buffer);
        
        let mut cutter = Cutter::new(input, output, 1000, 2000);
        
        let result = cutter.process_event(&event).unwrap();
        assert!(result, "Event within time range should be processed");
    }

    #[test]
    fn test_process_event_outside_range() {
        // Create a test event outside the range
        let event = Event::new(
            500, // timestamp outside range
            ThreadRef::Inline { process_koid: 0, thread_koid: 0 },
            StringRef::Inline("test_cat".to_string()),
            StringRef::Inline("test".to_string()),
            Vec::new(),
        );
        
        let mut input_buffer = Vec::new();
        let input = Cursor::new(&mut input_buffer);
        let mut output_buffer = Vec::new();
        let output = Cursor::new(&mut output_buffer);
        
        let mut cutter = Cutter::new(input, output, 1000, 2000);
        
        let result = cutter.process_event(&event).unwrap();
        assert!(!result, "Event outside time range should be filtered out");
    }

    #[test]
    fn test_empty_input() {
        // Test with empty input
        let empty_data = Vec::new();
        let input_reader = Cursor::new(empty_data);
        let mut output_buffer = Vec::new();
        let output_writer = Cursor::new(&mut output_buffer);
        
        let mut cutter = Cutter::new(input_reader, output_writer, 1000, 2000);
        let result = cutter.cut();
        
        assert!(result.is_ok(), "Cutting empty input should not error");
        assert_eq!(output_buffer.len(), 0, "Output should be empty for empty input");
    }
}
