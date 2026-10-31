# Track C: Join — Fast Zero-Dependency Rust Relational Engine

## Approach
- **Zero-Copy Streaming CSV Parser**:
  - Direct pointer-based state machine for hostile CSV parsing (quoted commas, escaped quotes `""`, embedded newlines, mixed CRLF/LF, and mid-file BOM markers).
  - Handles the leading and mid-record byte order marks (`EF BB BF`) deterministically.
- **Constant-Time $O(1)$ Region Lookup**:
  - Mapped region IDs to country IDs using a flat 10 MB direct array `region_table[region_id]`, turning hash join into a 1-cycle direct memory lookup.
- **Zero-Allocation Name & Note Processing**:
  - Notes are byte-counted during scan without storing text or allocating memory.
  - Names with unescaped text are read directly as slices from the mmapped input buffer. A scratch buffer is used only when `""` escaping is encountered.
  - Distinct names per country are maintained via a compact open-addressing hash set.
- **Exact Fixed-Point Arithmetic**:
  - Amounts parsed directly into 64-bit integer cents to completely avoid floating-point errors.
- **Zero Runtime Dependencies**:
  - Pure standard library Rust ($D = 1.0$), ensuring zero dependency weight penalty.

## Dependencies
None (0 non-stdlib runtime dependencies).
