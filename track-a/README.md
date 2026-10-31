# Track A: Ingest — Fast Zero-Dependency Rust Solution

## Approach
- **Zero-Copy / Chunk-Based Parallel Parsing**: The 239 MB log file is split across up to 12 hardware threads at newline boundaries.
- **Fast Custom Tokenization**:
  - Timestamps are truncated to the UTC hour by slicing the first 13 characters (`YYYY-MM-DDTHH`) without date library overhead.
  - Method is skipped directly.
  - Endpoint byte slices are parsed without decoding.
  - HTTP status is checked for error (`5xx`) via direct digit inspection.
  - Latencies are parsed via fast ASCII-to-integer conversion.
- **Percentile Computation**: Group latencies are sorted in-place and extracted via exact 1-indexed rank formulas: `ceil(p * n / 100) = (p * n + 99) / 100`.
- **Zero Runtime Dependencies**: Standard library only, ensuring dependency factor $D = 1.0$.

## Dependencies
None (0 non-stdlib runtime dependencies).

## What We Would Do With Another 24 Hours
- Implement a custom open-addressing Swiss-table style hash map with SIMD probing for even faster group lookup.
- Fast radix sort or QuickSelect (`select_nth_unstable`) for groups with large latency counts instead of full sort.
