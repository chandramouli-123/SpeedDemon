# Track A: Ingest — Optimization Log

- **Step 0 — Naive Reference**:
  - Python 3.13 naive implementation with `datetime.strptime()` per line and global dictionary appending.
  - Measured baseline: 32.05 s wall time, 228 MB peak memory.

- **Step 1 — Zero-Dependency Rust Port (run-da987b456889)**:
  - Replaced Python interpreter with optimized Rust binary (`opt-level = 3`, LTO enabled).
  - Truncate timestamps directly from ASCII byte slices without datetime library parsing.
  - Chunk-based parallel ingestion across 12 threads with per-thread standard `HashMap`.
  - Board measurement: **0.357 s** wall time, **315 MB** peak memory, **K = 7.57** (43.96 pts).
  - All 5 gates passed (correctness, timeout, sandbox, check, build).

- **Step 2 — Zero-Copy mmap + Lock-Free Open-Addressing Interner + Parallel Percentiles**:
  - Switched input ingestion to `mmap` with `MAP_SHARED`, eliminating the 239 MB userspace heap buffer allocation.
  - Replaced dynamic heap allocations of endpoints with a thread-local 4096-slot open-addressing table.
  - SWAR (64-bit word) character scanning for ultra-low latency delimiter parsing.
  - Single contiguous latency array with prefix-sum scatter, completely eliminating heap fragmentation.
  - Multi-threaded percentile selection and formatting across 12 threads.
  - Local benchmark on full 3.1M records: **0.040 s** (~9x faster than Step 1), memory reduced to **<50 MB**.
