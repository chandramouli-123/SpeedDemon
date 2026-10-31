# Track A: Ingest — Optimization Log

- **Step 0 — Naive Reference**:
  - Python 3.13 naive implementation with `datetime.strptime()` per line and global dictionary appending.
  - Measured baseline: 32.05 s wall time, 228 MB peak memory.
- **Step 1 — Zero-Dependency Rust Port**:
  - Replaced Python interpreter with optimized Rust binary (`opt-level = 3`, LTO enabled).
  - Truncate timestamps directly from ASCII byte slices without datetime library parsing.
  - Custom ASCII integer parsing for status and latencies.
- **Step 2 — Parallel Chunk Ingestion**:
  - Partitioned input buffer across hardware threads (up to 12 threads matching the Falkenstein 6 P-cores).
  - Independent thread-local hash maps merged at completion to eliminate lock contention during parsing.
- **Step 3 — Formatter and Dependency Cleanliness**:
  - 0 non-stdlib dependencies ($D = 1.0$).
  - Clean, idiomatic Rust formatted for compact, maintainable line count.
