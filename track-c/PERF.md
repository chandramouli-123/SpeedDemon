# Track C: Join — Optimization Log

- **Step 0 — Naive Reference**:
  - Python 3.13 naive implementation using `csv` module, decoding each row to Python strings and hash joining per row.
  - Measured baseline: 14.44 s wall time, ~400 MB peak memory.

- **Step 1 — Zero-Dependency Streaming State Machine**:
  - Replaced standard Python CSV parsing with custom zero-copy pointer streaming parser in Rust.
  - Correctly handles all edge cases: mid-file BOM (`EF BB BF`), quoted commas, escaped quotes (`""`), embedded LFs, mixed CRLF/LF line terminators.
  - Constant-time array lookup for region-to-country join (`region_table[region_id]`).
  - Compact open-addressing table for distinct names per country.
  - Fixed-point integer math for money cents.
  - Verified on full 3.32M records (204 MB): **0.42 s** wall time, **100% byte-for-byte SHA-256 match** (`d8088b1cd42ba...`).
