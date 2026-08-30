//! The crate's shared integration-test binary.
//!
//! Cargo compiles every `tests/*.rs` file into its own executable, each
//! statically linking the whole dependency graph — the escurel gateway and
//! libduckdb included. Files that sit *inside* `tests/suite/` are not
//! targets in their own right, so declaring them as modules here collapses
//! them into one binary.
//!
//! The workspace's largest consumer of link time and disk by a wide margin:
//! 17 test files at ~490 MB each, 8.8 GB of the 15.3 GB total.
//!
//! Adding a test file: put it in `tests/suite/` and add its `mod` line
//! below. A file that is not listed here is silently not compiled — nothing
//! warns you about it.
//!
//! The layout matters: it must be `tests/suite/main.rs`, not
//! `tests/suite.rs`. A test target's root file resolves `mod x;` against
//! its *own* directory, so `tests/suite.rs` would look for `tests/x.rs`.

mod compose_unit;
mod data_northwind;
mod document_view;
mod embedded_face;
mod instance_card_png;
mod instance_timeline;
mod instance_views;
mod mosaic_northwind;
mod no_credential_surface;
mod render_northwind;
mod salesperson_leaderboard;
mod saved_instances;
mod shared_selection;
mod stat_compose;
mod stat_render_feature_off;
mod stat_render_ggplot;
mod supplier_lead_time;
