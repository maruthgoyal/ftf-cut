# ftf-cut

A Rust CLI tool to filter Fuchsia Trace Format (FTF) traces by timestamp range.

Can process 2Gi traces at 300-900MB/s with <2MB memory usage
## Usage

```bash
ftf-cut --start-ts <START_TS> --end-ts <END_TS> --input-path <INPUT_PATH> --output-path <OUTPUT_PATH>
```

Where:
- `START_TS`: The start timestamp (inclusive)
- `END_TS`: The end timestamp (inclusive)
- `INPUT_PATH`: Path to the input FTF trace file
- `OUTPUT_PATH`: Path where the filtered trace file will be written

## How It Works

The tool reads an FTF trace file and:
1. Filters event records to only include those with timestamps between `START_TS` and `END_TS`
2. Preserves all string records that are referenced by the included events
3. Omits string records that are only referenced by excluded events
4. Copies all other record types unchanged

This effectively creates a smaller trace file focused only on the events in the time range of interest.

## Benchmarking

The repository includes tools for benchmarking the performance and size reduction of ftf-cut:

### Generating a Large Test Trace

```bash
cargo run --release --example generate_large_trace
```

This creates a ~1-2 GB trace file with 50 million events and string records added periodically.

### Running the Benchmark

```bash
cargo run --release --example benchmark
```

This runs ftf-cut on the generated trace with different time ranges and reports:
- Processing time
- Input and output file sizes
- Size reduction percentage
- Processing speed (MB/s and events/second)

## Running Tests

```bash
cargo test
```