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

- **Step 2 — Zero-Copy mmap + SWAR Scanner + Parallel Percentiles (run-39bebfee1020)**:
  - Switched input ingestion to `mmap` with `MAP_SHARED`, eliminating the 239 MB userspace heap buffer allocation.
  - SWAR (64-bit word) character scanning for ultra-low latency delimiter parsing.
  - Board measurement: **0.103 s** wall time, **55 MB** peak memory, **K = 18.37** (**63.20 pts**).

- **Step 3 — 3-Instruction Fast-Hash Table + Parallel Scatter + Target Native Vectorization**:
  - Replaced byte-by-byte endpoint hashing with a 3-instruction 64-bit prefix/suffix word hash.
  - Expanded table to 16,384 slots (<12% load factor) with direct prefix caching for instant hit rates.
  - Converted scattering into a parallel disjoint scatter per thread, cutting intermediate aggregation overhead.
  - Enabled `-C target-cpu=native` for full AVX2 vectorization on the scoring host.
  - Local benchmark on full 3.1M records: **0.046 s** wall time, **100% SHA-256 match**.
