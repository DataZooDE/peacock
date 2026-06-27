//! The `peacock` binary: CLI / `PEACOCK_*` settings / manifest loader and
//! the process entrypoint (listeners + signal handler). Logic lands in
//! Phase 7; this is the scaffold so the workspace produces a binary.

fn main() {
    println!("peacock {}", peacock_types::VERSION);
}
