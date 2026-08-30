//! The crate's shared integration-test binary.
//!
//! Cargo compiles every `tests/*.rs` file into its own executable, each
//! statically linking the whole dependency graph — the escurel gateway and
//! libduckdb included. Files that sit *inside* `tests/suite/` are not
//! targets in their own right, so declaring them as modules here collapses
//! them into one binary.
//!
//! Two files, but each linked the full graph at ~493 MB to drive the binary.
//!
//! Adding a test file: put it in `tests/suite/` and add its `mod` line
//! below. A file that is not listed here is silently not compiled — nothing
//! warns you about it.
//!
//! The layout matters: it must be `tests/suite/main.rs`, not
//! `tests/suite.rs`. A test target's root file resolves `mod x;` against
//! its *own* directory, so `tests/suite.rs` would look for `tests/x.rs`.

mod author;
mod lifecycle;
