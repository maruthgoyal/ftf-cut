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
                    let jump = (header.size() - 1) * 8;
                    self.input.seek_relative(jump.into())?;
                }
                RecordType::Event => {
                    // self.input.seek(std::io::SeekFrom::Start(pos))?;
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
            self.input.seek_relative((*offset as i64) - (pos as i64))?;

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

            self.input.seek_relative((pos - *offset) as i64)?;
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
